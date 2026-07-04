use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose};
use serde::Deserialize;
use serde_json::{Value, json};

#[cfg(test)]
use crate::HttpResponse;
use crate::{GenerationError, HttpTransport, UreqTransport};

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ImageConfig {
    pub budget_per_run: usize,
    pub provider: ImageProviderKind,
    pub model: String,
    pub endpoint_base: String,
    pub aspect_ratio: Option<String>,
    pub image_size: Option<String>,
}

impl Default for ImageConfig {
    fn default() -> Self {
        Self {
            budget_per_run: 4,
            provider: ImageProviderKind::Gemini,
            model: "gemini-3.1-flash-lite-image".to_owned(),
            endpoint_base: "https://generativelanguage.googleapis.com/v1beta".to_owned(),
            aspect_ratio: Some("16:9".to_owned()),
            image_size: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ImageProviderKind {
    Mock,
    #[default]
    Gemini,
    GptImage2,
}

#[derive(Debug, Clone)]
pub struct ImageRequest {
    pub prompt: String,
    pub alt: String,
    pub model: String,
    pub aspect_ratio: Option<String>,
    pub image_size: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageOutput {
    pub bytes: Vec<u8>,
    pub mime_type: String,
    pub provider: String,
    pub model: String,
    pub spend_micros: u64,
}

pub trait ImageProvider: Send + Sync {
    fn render(&self, request: &ImageRequest) -> Result<ImageOutput, GenerationError>;
}

#[derive(Debug, Clone)]
pub struct MockImageProvider;

impl ImageProvider for MockImageProvider {
    fn render(&self, request: &ImageRequest) -> Result<ImageOutput, GenerationError> {
        Ok(ImageOutput {
            bytes: vec![
                137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0,
                1, 8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248,
                207, 192, 240, 31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68,
                174, 66, 96, 130,
            ],
            mime_type: "image/png".to_owned(),
            provider: "mock-image".to_owned(),
            model: request.model.clone(),
            spend_micros: 0,
        })
    }
}

pub struct GeminiImageProvider {
    api_key: String,
    endpoint_base: String,
    transport: Arc<dyn HttpTransport>,
}

impl GeminiImageProvider {
    pub fn from_env(config: &ImageConfig) -> Result<Self, GenerationError> {
        let api_key =
            std::env::var("GEMINI_API_KEY").map_err(|_| GenerationError::MissingSecret {
                name: "GEMINI_API_KEY",
            })?;
        Ok(Self {
            api_key,
            endpoint_base: config.endpoint_base.clone(),
            transport: Arc::new(UreqTransport),
        })
    }

    pub fn new(api_key: String, endpoint_base: String, transport: Arc<dyn HttpTransport>) -> Self {
        Self {
            api_key,
            endpoint_base,
            transport,
        }
    }
}

impl ImageProvider for GeminiImageProvider {
    fn render(&self, request: &ImageRequest) -> Result<ImageOutput, GenerationError> {
        let endpoint = format!("{}/interactions", self.endpoint_base.trim_end_matches('/'));
        let body = image_request_body(request);
        let headers = vec![("x-goog-api-key", self.api_key.clone())];
        let response = self.transport.post_json(&endpoint, &headers, &body)?;
        if response.status >= 400 {
            return Err(GenerationError::Provider {
                provider: "gemini-image",
                retryable: matches!(response.status, 408 | 429 | 500..=599),
                message: format!(
                    "http {}: {}",
                    response.status,
                    response.body.chars().take(500).collect::<String>()
                ),
            });
        }

        let value = serde_json::from_str::<Value>(&response.body)
            .map_err(|error| GenerationError::InvalidProviderResponse(error.to_string()))?;
        extract_generated_image(&value, &request.model)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageBudget {
    remaining: usize,
}

impl ImageBudget {
    pub fn new(limit: usize) -> Self {
        Self { remaining: limit }
    }

    fn take(&mut self) -> bool {
        if self.remaining == 0 {
            return false;
        }
        self.remaining -= 1;
        true
    }

    pub fn remaining(&self) -> usize {
        self.remaining
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRenderReport {
    pub html: String,
    pub requested: usize,
    pub rendered: usize,
    pub failed: usize,
    pub skipped: usize,
    pub spend_micros: u64,
    pub files: Vec<PathBuf>,
    pub messages: Vec<String>,
}

pub fn render_image_requests(
    html: &str,
    output_dir: &Path,
    config: &ImageConfig,
    provider: &dyn ImageProvider,
    budget: &mut ImageBudget,
) -> ImageRenderReport {
    let mut rendered_html = String::with_capacity(html.len());
    let mut cursor = 0;
    let mut requested = 0;
    let mut rendered = 0;
    let mut failed = 0;
    let mut skipped = 0;
    let mut spend_micros = 0u64;
    let mut files = Vec::new();
    let mut messages = Vec::new();

    while let Some(offset) = html[cursor..].find("<figure") {
        let start = cursor + offset;
        let Some(open_end) = html[start..].find('>').map(|index| start + index + 1) else {
            break;
        };
        let opening = &html[start..open_end];
        let Some(prompt) = attr_value(opening, "data-glance-image-prompt") else {
            rendered_html.push_str(&html[cursor..open_end]);
            cursor = open_end;
            continue;
        };
        let Some(close_offset) = html[open_end..].find("</figure>") else {
            rendered_html.push_str(&html[cursor..open_end]);
            cursor = open_end;
            continue;
        };
        let close_end = open_end + close_offset + "</figure>".len();
        let alt = attr_value(opening, "data-glance-image-alt")
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "Requested Glance illustration".to_owned());
        requested += 1;
        rendered_html.push_str(&html[cursor..start]);

        if !budget.take() {
            skipped += 1;
            let fallback = fallback_figure(&prompt, &alt, "image budget exhausted");
            rendered_html.push_str(&fallback);
            messages.push(format!(
                "skipped image request {requested}: budget exhausted"
            ));
            cursor = close_end;
            continue;
        }

        let request = ImageRequest {
            prompt: prompt.clone(),
            alt: alt.clone(),
            model: config.model.clone(),
            aspect_ratio: config.aspect_ratio.clone(),
            image_size: config.image_size.clone(),
        };
        match provider.render(&request) {
            Ok(output) => {
                spend_micros = spend_micros.saturating_add(output.spend_micros);
                let extension = extension_for_mime(&output.mime_type);
                let filename = format!("glance-image-{requested:03}.{extension}");
                let path = output_dir.join(&filename);
                match std::fs::create_dir_all(output_dir)
                    .and_then(|()| std::fs::write(&path, &output.bytes))
                {
                    Ok(()) => {
                        rendered += 1;
                        files.push(path);
                        messages.push(format!(
                            "rendered image request {requested}: provider={} model={} file={filename}",
                            output.provider, output.model
                        ));
                        rendered_html.push_str(&rendered_figure(&filename, &alt));
                    }
                    Err(error) => {
                        failed += 1;
                        messages.push(format!("failed image request {requested}: {error}"));
                        rendered_html.push_str(&fallback_figure(&prompt, &alt, &error.to_string()));
                    }
                }
            }
            Err(error) => {
                failed += 1;
                messages.push(format!("failed image request {requested}: {error}"));
                rendered_html.push_str(&fallback_figure(&prompt, &alt, &error.to_string()));
            }
        }
        cursor = close_end;
    }

    rendered_html.push_str(&html[cursor..]);
    ImageRenderReport {
        html: rendered_html,
        requested,
        rendered,
        failed,
        skipped,
        spend_micros,
        files,
        messages,
    }
}

fn image_request_body(request: &ImageRequest) -> Value {
    let mut response_format = serde_json::Map::new();
    response_format.insert("type".to_owned(), json!("image"));
    if let Some(aspect_ratio) = &request.aspect_ratio {
        response_format.insert("aspect_ratio".to_owned(), json!(aspect_ratio));
    }
    if let Some(image_size) = &request.image_size {
        response_format.insert("image_size".to_owned(), json!(image_size));
    }

    json!({
        "model": request.model.trim_start_matches("models/"),
        "input": request.prompt,
        "response_format": response_format
    })
}

fn extract_generated_image(value: &Value, model: &str) -> Result<ImageOutput, GenerationError> {
    if let Some(output_image) = value
        .get("output_image")
        .or_else(|| value.get("outputImage"))
    {
        return image_output_from_value(output_image, model, "output_image");
    }

    if let Some(steps) = value.get("steps").and_then(Value::as_array) {
        for step in steps {
            let Some(content) = step.get("content").and_then(Value::as_array) else {
                continue;
            };
            for item in content {
                if item.get("type").and_then(Value::as_str) == Some("image")
                    && item.get("data").is_some()
                {
                    return image_output_from_value(item, model, "steps.content.image");
                }
            }
        }
    }

    let parts = value["candidates"][0]["content"]["parts"]
        .as_array()
        .ok_or_else(|| {
            GenerationError::InvalidProviderResponse(
                "output_image, steps.content image, or candidates[0].content.parts".to_owned(),
            )
        })?;
    for part in parts {
        let inline = part.get("inlineData").or_else(|| part.get("inline_data"));
        let Some(inline) = inline else {
            continue;
        };
        return image_output_from_value(inline, model, "inlineData");
    }
    Err(GenerationError::InvalidProviderResponse(
        "output_image, steps.content image, or candidate image inlineData".to_owned(),
    ))
}

fn image_output_from_value(
    image: &Value,
    model: &str,
    label: &str,
) -> Result<ImageOutput, GenerationError> {
    let data = image
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| GenerationError::InvalidProviderResponse(format!("{label}.data")))?;
    let mime_type = image
        .get("mimeType")
        .or_else(|| image.get("mime_type"))
        .and_then(Value::as_str)
        .unwrap_or("image/png")
        .to_owned();
    let bytes = general_purpose::STANDARD
        .decode(data)
        .map_err(|error| GenerationError::InvalidProviderResponse(error.to_string()))?;
    Ok(ImageOutput {
        bytes,
        mime_type,
        provider: "gemini-image".to_owned(),
        model: model.to_owned(),
        spend_micros: 0,
    })
}

fn rendered_figure(filename: &str, alt: &str) -> String {
    format!(
        r#"<figure class="glance-image"><img src="{}" alt="{}"><figcaption>{}</figcaption></figure>"#,
        html_escape(filename),
        html_escape(alt),
        html_escape(alt)
    )
}

fn fallback_figure(prompt: &str, alt: &str, reason: &str) -> String {
    format!(
        r#"<figure class="glance-image-request" data-glance-image-prompt="{}" data-glance-image-alt="{}"><div class="glance-image-fallback" role="img" aria-label="{}">{}</div><figcaption>Image not rendered: {}</figcaption></figure>"#,
        html_escape(prompt),
        html_escape(alt),
        html_escape(alt),
        html_escape(alt),
        html_escape(reason)
    )
}

fn attr_value(opening_tag: &str, name: &str) -> Option<String> {
    let offset = opening_tag.find(name)?;
    let bytes = opening_tag.as_bytes();
    let mut index = offset + name.len();
    while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
        index += 1;
    }
    if bytes.get(index) != Some(&b'=') {
        return None;
    }
    index += 1;
    while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
        index += 1;
    }
    let quote = *bytes.get(index)?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let value_start = index + 1;
    let value_end = opening_tag[value_start..]
        .find(char::from(quote))
        .map(|relative| value_start + relative)?;
    Some(html_unescape(&opening_tag[value_start..value_end]))
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn html_unescape(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn extension_for_mime(mime_type: &str) -> &'static str {
    match mime_type {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        _ => "png",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    #[test]
    fn mock_provider_renders_requested_figure_beside_page() {
        let output_dir = tempfile::tempdir().expect("output");
        let mut budget = ImageBudget::new(1);
        let html = r#"<!doctype html><figure data-glance-image-prompt="draw the map" data-glance-image-alt="Map"></figure>"#;

        let report = render_image_requests(
            html,
            output_dir.path(),
            &ImageConfig::default(),
            &MockImageProvider,
            &mut budget,
        );

        assert_eq!(report.requested, 1);
        assert_eq!(report.rendered, 1);
        assert!(
            report
                .html
                .contains(r#"<img src="glance-image-001.png" alt="Map">"#)
        );
        assert!(output_dir.path().join("glance-image-001.png").is_file());
    }

    #[test]
    fn render_failure_keeps_fallback_without_broken_img() {
        let output_dir = tempfile::tempdir().expect("output");
        let mut budget = ImageBudget::new(1);
        let provider = FailingImageProvider;
        let html = r#"<!doctype html><figure data-glance-image-prompt="draw" data-glance-image-alt="Diagram"></figure>"#;

        let report = render_image_requests(
            html,
            output_dir.path(),
            &ImageConfig::default(),
            &provider,
            &mut budget,
        );

        assert_eq!(report.failed, 1);
        assert!(report.html.contains("glance-image-fallback"));
        assert!(report.html.contains("Diagram"));
        assert!(!report.html.contains("<img "));
    }

    #[test]
    fn gemini_image_provider_uses_interactions_response_format_and_reads_output_image() {
        let png = general_purpose::STANDARD.encode([1_u8, 2, 3]);
        let transport = Arc::new(StaticTransport {
            response: Mutex::new(Some(HttpResponse {
                status: 200,
                body: json!({
                    "steps": [{
                        "type": "model_output",
                        "content": [{
                            "type": "image",
                            "mime_type": "image/png",
                            "data": png
                        }]
                    }]
                })
                .to_string(),
            })),
            requests: Mutex::new(Vec::new()),
        });
        let provider = GeminiImageProvider::new(
            "key".to_owned(),
            "http://127.0.0.1/v1beta".to_owned(),
            transport.clone(),
        );
        let request = ImageRequest {
            prompt: "draw".to_owned(),
            alt: "Drawing".to_owned(),
            model: "gemini-3.1-flash-lite-image".to_owned(),
            aspect_ratio: Some("16:9".to_owned()),
            image_size: None,
        };

        let output = provider.render(&request).expect("image output");

        assert_eq!(output.bytes, vec![1, 2, 3]);
        let requests = transport.requests.lock().expect("requests");
        assert_eq!(requests[0].0, "http://127.0.0.1/v1beta/interactions");
        let body = &requests[0].1;
        assert_eq!(body["model"], "gemini-3.1-flash-lite-image");
        assert_eq!(body["input"], "draw");
        assert_eq!(body["response_format"]["type"], "image");
        assert_eq!(body["response_format"]["aspect_ratio"], "16:9");
    }

    #[test]
    fn live_smoke_renders_one_gemini_image_when_enabled() {
        if std::env::var("GLANCE_LIVE_IMAGE_SMOKE").ok().as_deref() != Some("1") {
            eprintln!("skipping live image smoke; set GLANCE_LIVE_IMAGE_SMOKE=1");
            return;
        }

        let config = ImageConfig {
            budget_per_run: 1,
            ..ImageConfig::default()
        };
        let provider = GeminiImageProvider::from_env(&config).expect("GEMINI_API_KEY");
        let request = ImageRequest {
            prompt: "Create a concise, non-photoreal architecture illustration for a static documentation generator: source tree, tier prompts, citation gate, and published HTML pages connected by labeled arrows. Use clean geometry and no tiny text."
                .to_owned(),
            alt: "Glance architecture smoke-test illustration".to_owned(),
            model: config.model.clone(),
            aspect_ratio: config.aspect_ratio.clone(),
            image_size: config.image_size.clone(),
        };

        let output = provider.render(&request).expect("live image output");

        println!(
            "live_image_smoke provider={} model={} mime={} bytes={}",
            output.provider,
            output.model,
            output.mime_type,
            output.bytes.len()
        );
        assert!(output.mime_type.starts_with("image/"));
        assert!(output.bytes.len() > 1_000);
    }

    struct FailingImageProvider;

    impl ImageProvider for FailingImageProvider {
        fn render(&self, _request: &ImageRequest) -> Result<ImageOutput, GenerationError> {
            Err(GenerationError::Provider {
                provider: "test-image",
                retryable: false,
                message: "planned failure".to_owned(),
            })
        }
    }

    type RecordedImageRequest = (String, Value, BTreeMap<String, String>);

    struct StaticTransport {
        response: Mutex<Option<HttpResponse>>,
        requests: Mutex<Vec<RecordedImageRequest>>,
    }

    impl HttpTransport for StaticTransport {
        fn post_json(
            &self,
            url: &str,
            headers: &[(&str, String)],
            body: &Value,
        ) -> Result<HttpResponse, GenerationError> {
            self.requests.lock().expect("requests").push((
                url.to_owned(),
                body.clone(),
                headers
                    .iter()
                    .map(|(name, value)| ((*name).to_owned(), value.clone()))
                    .collect(),
            ));
            self.response
                .lock()
                .expect("response")
                .take()
                .ok_or_else(|| GenerationError::Http("no response".to_owned()))
        }
    }
}

#[cfg(test)]
use std::collections::BTreeMap;
use std::fmt;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::{Value, json};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelTier {
    Cheap,
    Mid,
    Frontier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKind {
    Leaf,
    Interior,
    Root,
    CrossCutting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierRouter {
    pub leaf: ModelTier,
    pub interior: ModelTier,
    pub root: ModelTier,
}

impl Default for TierRouter {
    fn default() -> Self {
        Self {
            leaf: ModelTier::Cheap,
            interior: ModelTier::Mid,
            root: ModelTier::Frontier,
        }
    }
}

impl TierRouter {
    pub fn tier_for(&self, kind: PageKind) -> ModelTier {
        match kind {
            PageKind::Leaf => self.leaf,
            PageKind::Interior => self.interior,
            PageKind::Root | PageKind::CrossCutting => self.root,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationRequest {
    pub source_root: PathBuf,
    pub directory: PathBuf,
    pub source_sha: String,
    pub kind: PageKind,
    pub prompt_context: Option<PromptContext>,
}

impl GenerationRequest {
    pub fn new(
        source_root: PathBuf,
        directory: PathBuf,
        source_sha: String,
        kind: PageKind,
    ) -> Self {
        Self {
            source_root,
            directory,
            source_sha,
            kind,
            prompt_context: None,
        }
    }

    pub fn with_prompt_context(mut self, prompt_context: PromptContext) -> Self {
        self.prompt_context = Some(prompt_context);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedPage {
    pub html: String,
    pub prompt_version: String,
    pub tier: ModelTier,
    pub provider: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub spend_micros: u64,
    pub metadata_notes: Vec<String>,
    pub degraded_children: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpendReport {
    pub total_pages: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub spend_micros: u64,
    pub pages: Vec<PageSpend>,
}

impl Default for SpendReport {
    fn default() -> Self {
        Self::empty()
    }
}

impl SpendReport {
    fn empty() -> Self {
        Self {
            total_pages: 0,
            input_tokens: 0,
            output_tokens: 0,
            spend_micros: 0,
            pages: Vec::new(),
        }
    }

    pub fn record(&mut self, page: PageSpend) {
        self.total_pages += 1;
        self.input_tokens += page.input_tokens;
        self.output_tokens += page.output_tokens;
        self.spend_micros += page.spend_micros;
        self.pages.push(page);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageSpend {
    pub directory: PathBuf,
    pub provider: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub spend_micros: u64,
}

mod context;
pub use context::{
    PromptContext, assemble_prompt_context, assemble_prompt_context_with_degraded,
    normalize_generated_html_citations,
};
use context::{is_retryable_output_validation, validate_provider_output};

#[derive(Debug, Error)]
pub enum GenerationError {
    #[error("generation provider is scaffold-only: {0}")]
    ScaffoldOnly(String),
    #[error("generation budget exceeded: {message}")]
    BudgetExceeded { message: String },
    #[error("missing environment secret {name}")]
    MissingSecret { name: &'static str },
    #[error("{provider} provider failed: {message}")]
    Provider {
        provider: &'static str,
        retryable: bool,
        message: String,
    },
    #[error("provider response was missing expected field: {0}")]
    InvalidProviderResponse(String),
    #[error("context assembly failed: {message}")]
    Context { message: String },
    #[error("prompt template {path} is invalid: {message}")]
    PromptTemplate { path: &'static str, message: String },
    #[error("provider returned non-html output: {message}")]
    InvalidHtml { message: String },
    #[error("http transport failed: {0}")]
    Http(String),
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub trait PageGenerator {
    fn generate(&self, request: GenerationRequest) -> Result<GeneratedPage, GenerationError>;
}

#[derive(Debug, Clone, Default)]
pub struct MockProvider {
    router: TierRouter,
    routing: DepthRouting,
}

impl MockProvider {
    pub fn new(router: TierRouter) -> Self {
        Self {
            router,
            routing: DepthRouting::default(),
        }
    }

    pub fn with_routing(routing: DepthRouting) -> Self {
        Self {
            router: TierRouter::default(),
            routing,
        }
    }
}

impl PageGenerator for MockProvider {
    fn generate(&self, request: GenerationRequest) -> Result<GeneratedPage, GenerationError> {
        let prompt_context = PromptContext::from_request(&request, 64 * 1024)?;
        let tier = self.router.tier_for(request.kind);
        let route = self.routing.model_for(request.kind);
        let directory = request.directory.display();
        let citation = prompt_context.primary_citation.as_deref();
        let cited_attr = citation
            .map(|citation| format!(r#" class="glance-cited" data-glance-cite="{citation}""#))
            .unwrap_or_default();
        let cross_cutting = if matches!(request.kind, PageKind::Root | PageKind::CrossCutting) {
            format!(
                r#"<section class="glance-section glance-cross-cutting" data-glance-section="flows"><h2 class="glance-section-title">Flows</h2><p{cited_attr}>Mock flow across generated context.</p></section>
<section class="glance-section glance-cross-cutting" data-glance-section="data-model"><h2 class="glance-section-title">Data model</h2><p{cited_attr}>Mock data model distinguishes stored source from generated pages.</p></section>
<section class="glance-section glance-cross-cutting" data-glance-section="failure-edge-index"><h2 class="glance-section-title">Failure-edge index</h2><p{cited_attr}>Mock failure index carries child sharp edges.</p></section>"#
            )
        } else {
            String::new()
        };
        Ok(GeneratedPage {
            html: format!(
                r#"<!doctype html><html data-source-sha="{}" data-prompt-version="{}"><head><meta charset="utf-8"><title>Glance {directory}</title></head><body class="glance-page"><header class="glance-header"><h1>{directory}</h1></header><main>
<section class="glance-section" data-glance-section="what-this-is"><h2 class="glance-section-title">What this is</h2><p{cited_attr}>Mock glance page for {directory}.</p></section>
<section class="glance-section" data-glance-section="role-in-the-whole"><h2 class="glance-section-title">Role in the whole</h2><p{cited_attr}>Mock role in the repository.</p></section>
<section class="glance-section" data-glance-section="composition"><h2 class="glance-section-title">Composition</h2><div class="glance-composition"><p{cited_attr}>Mock composition section.</p></div></section>
<section class="glance-section" data-glance-section="seams-contracts"><h2 class="glance-section-title">Seams and contracts</h2><p{cited_attr}>Mock seam section.</p></section>
<section class="glance-section" data-glance-section="where-it-can-hurt-you"><h2 class="glance-section-title">Where it can hurt you</h2><p{cited_attr}>Nothing sharp found.</p></section>
{cross_cutting}</main></body></html>"#,
                request.source_sha, prompt_context.prompt_version
            ),
            prompt_version: prompt_context.prompt_version,
            tier,
            provider: "mock".to_owned(),
            model: route.model.clone(),
            input_tokens: 0,
            output_tokens: 0,
            spend_micros: 0,
            metadata_notes: prompt_context.metadata_notes,
            degraded_children: prompt_context.degraded_children,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GenerationConfig {
    pub provider_mode: ProviderMode,
    pub routing: DepthRouting,
    pub budget: BudgetConfig,
    pub retry: RetryConfig,
    pub prompt: PromptConfig,
}

impl Default for GenerationConfig {
    fn default() -> Self {
        Self {
            provider_mode: ProviderMode::Mock,
            routing: DepthRouting::default(),
            budget: BudgetConfig::default(),
            retry: RetryConfig::default(),
            prompt: PromptConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderMode {
    Mock,
    Real,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DepthRouting {
    pub leaf: ModelRoute,
    pub interior: ModelRoute,
    pub root: ModelRoute,
}

impl Default for DepthRouting {
    fn default() -> Self {
        Self {
            leaf: ModelRoute {
                tier: ModelTier::Cheap,
                provider: ProviderKind::OpenRouter,
                model: "deepseek/deepseek-v4-flash".to_owned(),
                max_tokens: 6_000,
                input_micros_per_million_tokens: 140_000,
                output_micros_per_million_tokens: 280_000,
            },
            interior: ModelRoute {
                tier: ModelTier::Mid,
                provider: ProviderKind::OpenRouter,
                model: "anthropic/claude-sonnet-5".to_owned(),
                max_tokens: 10_000,
                input_micros_per_million_tokens: 2_000_000,
                output_micros_per_million_tokens: 10_000_000,
            },
            root: ModelRoute {
                tier: ModelTier::Frontier,
                provider: ProviderKind::OpenRouter,
                model: "openai/gpt-5.5".to_owned(),
                max_tokens: 16_000,
                input_micros_per_million_tokens: 5_000_000,
                output_micros_per_million_tokens: 30_000_000,
            },
        }
    }
}

impl DepthRouting {
    pub fn model_for(&self, kind: PageKind) -> &ModelRoute {
        match kind {
            PageKind::Leaf => &self.leaf,
            PageKind::Interior => &self.interior,
            PageKind::Root | PageKind::CrossCutting => &self.root,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ModelRoute {
    pub tier: ModelTier,
    pub provider: ProviderKind,
    pub model: String,
    pub max_tokens: u32,
    pub input_micros_per_million_tokens: u64,
    pub output_micros_per_million_tokens: u64,
}

impl ModelRoute {
    fn estimate_cost_micros(&self, input_tokens: u64, output_tokens: u64) -> u64 {
        cost_micros(input_tokens, self.input_micros_per_million_tokens)
            + cost_micros(output_tokens, self.output_micros_per_million_tokens)
    }
}

impl Default for ModelRoute {
    fn default() -> Self {
        DepthRouting::default().leaf
    }
}

fn cost_micros(tokens: u64, micros_per_million: u64) -> u64 {
    tokens
        .saturating_mul(micros_per_million)
        .div_ceil(1_000_000)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    #[default]
    OpenRouter,
    Gemini,
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderKind::OpenRouter => formatter.write_str("openrouter"),
            ProviderKind::Gemini => formatter.write_str("gemini"),
        }
    }
}

impl<'de> Deserialize<'de> for ModelTier {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "cheap" => Ok(ModelTier::Cheap),
            "mid" => Ok(ModelTier::Mid),
            "frontier" => Ok(ModelTier::Frontier),
            _ => Err(serde::de::Error::custom(format!(
                "unknown model tier {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(default)]
pub struct BudgetConfig {
    pub per_run_micros: Option<u64>,
    pub per_day_micros: Option<u64>,
    pub spent_today_micros: u64,
}

#[derive(Debug, Clone)]
pub struct BudgetTracker {
    config: BudgetConfig,
    reserved_micros: u64,
    report: SpendReport,
}

impl BudgetTracker {
    pub fn new(config: BudgetConfig) -> Self {
        Self {
            config,
            reserved_micros: 0,
            report: SpendReport::empty(),
        }
    }

    pub fn reserve(&mut self, estimated_micros: u64) -> Result<(), GenerationError> {
        let projected_run = self
            .report
            .spend_micros
            .saturating_add(self.reserved_micros)
            .saturating_add(estimated_micros);
        if let Some(limit) = self.config.per_run_micros
            && projected_run > limit
        {
            return Err(GenerationError::BudgetExceeded {
                message: format!(
                    "per-run budget would be {projected_run} micros, limit is {limit} micros"
                ),
            });
        }

        let projected_day = self.config.spent_today_micros.saturating_add(projected_run);
        if let Some(limit) = self.config.per_day_micros
            && projected_day > limit
        {
            return Err(GenerationError::BudgetExceeded {
                message: format!(
                    "per-day budget would be {projected_day} micros, limit is {limit} micros"
                ),
            });
        }

        self.reserved_micros = self.reserved_micros.saturating_add(estimated_micros);
        Ok(())
    }

    pub fn record(&mut self, page: PageSpend, reserved_micros: u64) {
        self.reserved_micros = self.reserved_micros.saturating_sub(reserved_micros);
        self.report.record(page);
    }

    pub fn report(&self) -> SpendReport {
        self.report.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct RetryConfig {
    pub max_attempts: usize,
    pub base_backoff_millis: u64,
    pub jitter_millis: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_backoff_millis: 200,
            jitter_millis: 75,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct PromptConfig {
    pub max_file_bytes: usize,
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self {
            max_file_bytes: 64 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderOutput {
    pub html: String,
    pub provider: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub spend_micros: Option<u64>,
    pub finish_reason: Option<String>,
}

pub trait ProviderClient: Send + Sync {
    fn name(&self) -> &'static str;
    fn kind(&self) -> ProviderKind;

    fn generate_once(
        &self,
        prompt: &PromptContext,
        route: &ModelRoute,
    ) -> Result<ProviderOutput, GenerationError>;
}

pub struct RealPageGenerator {
    routing: DepthRouting,
    prompt: PromptConfig,
    client: FallbackClient,
    budget: std::sync::Mutex<BudgetTracker>,
}

impl RealPageGenerator {
    pub fn from_env(config: GenerationConfig) -> Result<Self, GenerationError> {
        let transport: Arc<dyn HttpTransport> = Arc::new(UreqTransport);
        let mut providers: Vec<Box<dyn ProviderClient>> = Vec::new();
        if std::env::var("OPENROUTER_API_KEY").is_ok() {
            providers.push(Box::new(OpenRouterClient::from_env(transport.clone())?));
        }
        if std::env::var("GEMINI_API_KEY").is_ok() {
            providers.push(Box::new(GeminiClient::from_env(transport)?));
        }
        if providers.is_empty() {
            return Err(GenerationError::MissingSecret {
                name: "OPENROUTER_API_KEY or GEMINI_API_KEY",
            });
        }

        Ok(Self::new(config, providers))
    }

    pub fn new(config: GenerationConfig, providers: Vec<Box<dyn ProviderClient>>) -> Self {
        Self {
            routing: config.routing,
            prompt: config.prompt,
            client: FallbackClient::new(providers, config.retry),
            budget: std::sync::Mutex::new(BudgetTracker::new(config.budget)),
        }
    }

    pub fn spend_report(&self) -> SpendReport {
        self.budget.lock().expect("budget mutex").report()
    }
}

impl PageGenerator for RealPageGenerator {
    fn generate(&self, request: GenerationRequest) -> Result<GeneratedPage, GenerationError> {
        let route = self.routing.model_for(request.kind);
        let prompt = PromptContext::from_request(&request, self.prompt.max_file_bytes)?;
        let estimated_micros =
            route.estimate_cost_micros(prompt.estimated_input_tokens, u64::from(route.max_tokens));
        self.budget
            .lock()
            .expect("budget mutex")
            .reserve(estimated_micros)?;

        let output = match self.client.generate_once(&prompt, route) {
            Ok(output) => output,
            Err(error) => {
                self.release_reservation(estimated_micros);
                return Err(error);
            }
        };
        let output = match self.postprocess_output(output, &request) {
            Ok(output) => output,
            Err(error) if is_retryable_output_validation(&error) => {
                let prompt = prompt.with_retry_feedback(&error.to_string());
                let retry_output = match self.client.generate_once(&prompt, route) {
                    Ok(output) => output,
                    Err(error) => {
                        self.release_reservation(estimated_micros);
                        return Err(error);
                    }
                };
                match self.postprocess_output(retry_output, &request) {
                    Ok(output) => output,
                    Err(error) => {
                        self.release_reservation(estimated_micros);
                        return Err(error);
                    }
                }
            }
            Err(error) => {
                self.release_reservation(estimated_micros);
                return Err(error);
            }
        };

        let spend_micros = output.spend_micros.unwrap_or_else(|| {
            route.estimate_cost_micros(output.input_tokens, output.output_tokens)
        });
        let page_spend = PageSpend {
            directory: request.directory.clone(),
            provider: output.provider.clone(),
            model: output.model.clone(),
            input_tokens: output.input_tokens,
            output_tokens: output.output_tokens,
            spend_micros,
        };
        self.budget
            .lock()
            .expect("budget mutex")
            .record(page_spend, estimated_micros);

        Ok(GeneratedPage {
            html: output.html,
            prompt_version: prompt.prompt_version,
            tier: route.tier,
            provider: output.provider,
            model: output.model,
            input_tokens: output.input_tokens,
            output_tokens: output.output_tokens,
            spend_micros,
            metadata_notes: prompt.metadata_notes,
            degraded_children: prompt.degraded_children,
        })
    }
}

impl RealPageGenerator {
    fn postprocess_output(
        &self,
        mut output: ProviderOutput,
        request: &GenerationRequest,
    ) -> Result<ProviderOutput, GenerationError> {
        validate_provider_output(&output)?;
        output.html = normalize_generated_html_citations(
            &output.html,
            &request.source_root,
            &request.directory,
        )?;
        output.html = normalize_glance_directory_attribute(&output.html, &request.directory);
        output.html = ensure_required_navigation_links(&output.html, request)?;
        output.html = ensure_root_flow_diagram(&output.html, request)?;
        validate_generated_navigation(&output.html, request)?;
        Ok(output)
    }

    fn release_reservation(&self, estimated_micros: u64) {
        let mut budget = self.budget.lock().expect("budget mutex");
        budget.reserved_micros = budget.reserved_micros.saturating_sub(estimated_micros);
    }
}

fn normalize_glance_directory_attribute(html: &str, directory: &Path) -> String {
    let directory = html_escape(&path_label(directory));
    let mut output = String::with_capacity(html.len());
    let mut cursor = 0;
    let needle = "data-glance-directory";
    let bytes = html.as_bytes();

    while let Some(relative) = html[cursor..].find(needle) {
        let attr_start = cursor + relative;
        let mut index = attr_start + needle.len();
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if bytes.get(index) != Some(&b'=') {
            if bytes
                .get(index)
                .is_none_or(|byte| byte.is_ascii_whitespace() || matches!(byte, b'>' | b'/'))
            {
                output.push_str(&html[cursor..attr_start + needle.len()]);
                output.push_str(&format!("=\"{directory}\""));
                cursor = attr_start + needle.len();
                continue;
            }
            output.push_str(&html[cursor..index]);
            cursor = index;
            continue;
        }
        index += 1;
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        let Some(quote) = bytes.get(index).copied() else {
            output.push_str(&html[cursor..]);
            return output;
        };
        if quote != b'"' && quote != b'\'' {
            output.push_str(&html[cursor..=index]);
            cursor = index + 1;
            continue;
        }
        let value_start = index + 1;
        let Some(value_end) = html[value_start..]
            .find(char::from(quote))
            .map(|relative| value_start + relative)
        else {
            output.push_str(&html[cursor..]);
            return output;
        };

        output.push_str(&html[cursor..value_start]);
        output.push_str(&directory);
        output.push(char::from(quote));
        cursor = value_end + 1;
    }

    output.push_str(&html[cursor..]);
    output
}

fn ensure_required_navigation_links(
    html: &str,
    request: &GenerationRequest,
) -> Result<String, GenerationError> {
    let snapshot = glance_core::snapshot_tree(&request.source_root, request.source_sha.clone())
        .map_err(|error| GenerationError::Context {
            message: error.to_string(),
        })?;
    let Some(record) = snapshot.directory(&request.directory) else {
        return Ok(html.to_owned());
    };

    let mut links = Vec::new();
    if request.directory != Path::new(".") {
        let parent = request
            .directory
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        links.push(format!(
            r#"<a class="glance-nav-parent" href="{}">Parent</a>"#,
            html_escape(&glance_check::directory_href(&request.directory, parent))
        ));
    }
    let mut children = record.child_dirs.clone();
    children.sort();
    for child in children {
        links.push(format!(
            r#"<a class="glance-nav-child" href="{}">{}</a>"#,
            html_escape(&glance_check::directory_href(&request.directory, &child)),
            html_escape(&path_label(&child))
        ));
    }

    if links.is_empty() {
        return Ok(html.to_owned());
    }

    let nav = format!(
        r#"<nav class="glance-nav glance-nav-generated" aria-label="Generated navigation">{}</nav>"#,
        links.join("")
    );
    Ok(insert_after_opening_body(html, &nav).unwrap_or_else(|| html.to_owned()))
}

fn insert_after_opening_body(html: &str, insertion: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let body_start = lower.find("<body")?;
    let body_end = html[body_start..]
        .find('>')
        .map(|offset| body_start + offset + 1)?;
    let mut output = String::with_capacity(html.len() + insertion.len());
    output.push_str(&html[..body_end]);
    output.push_str(insertion);
    output.push_str(&html[body_end..]);
    Some(output)
}

fn ensure_root_flow_diagram(
    html: &str,
    request: &GenerationRequest,
) -> Result<String, GenerationError> {
    if request.directory != Path::new(".") || html.contains("glance-diagram") {
        return Ok(html.to_owned());
    }
    let snapshot = glance_core::snapshot_tree(&request.source_root, request.source_sha.clone())
        .map_err(|error| GenerationError::Context {
            message: error.to_string(),
        })?;
    let Some(root) = snapshot.directory(Path::new(".")) else {
        return Ok(html.to_owned());
    };
    let mut labels = root
        .child_dirs
        .iter()
        .map(|child| path_label(child))
        .collect::<Vec<_>>();
    labels.sort();
    let lanes = labels
        .iter()
        .take(6)
        .enumerate()
        .map(|(index, label)| {
            let x = 36 + (index as i32 * 96);
            format!(
                r#"<g class="glance-flow-lane"><rect x="{x}" y="38" width="78" height="34" rx="6"></rect><text x="{}" y="59">{}</text></g>"#,
                x + 39,
                html_escape(label)
            )
        })
        .collect::<String>();
    let diagram = format!(
        r#"<section class="glance-section glance-generated-flow"><style>@keyframes glance-flow-pulse{{0%{{opacity:.25;transform:translateX(0)}}50%{{opacity:1}}100%{{opacity:.25;transform:translateX(520px)}}}}@media (prefers-reduced-motion: reduce){{.glance-flow-dot{{display:none}}}}.glance-diagram text{{font:12px ui-monospace,monospace;text-anchor:middle;fill:currentColor}}.glance-diagram rect{{fill:transparent;stroke:currentColor;stroke-opacity:.35}}.glance-diagram path{{stroke:currentColor;stroke-opacity:.45;fill:none}}.glance-flow-dot{{animation:glance-flow-pulse 4s linear infinite;transform-box:fill-box}}</style><svg class="glance-diagram" viewBox="0 0 760 170" role="img" aria-label="Animated flow from source tree through prompts and checks into generated pages"><title>Glance generation flow</title><path d="M76 88 H684"></path>{lanes}<g><rect x="206" y="96" width="112" height="42" rx="7"></rect><text x="262" y="121">tier prompts</text></g><g><rect x="368" y="96" width="128" height="42" rx="7"></rect><text x="432" y="121">citation + nav check</text></g><g><rect x="548" y="96" width="116" height="42" rx="7"></rect><text x="606" y="121">published HTML</text></g><circle class="glance-flow-dot" cx="80" cy="88" r="6"></circle></svg></section>"#
    );
    Ok(insert_after_opening_body(html, &diagram).unwrap_or_else(|| html.to_owned()))
}

fn validate_generated_navigation(
    html: &str,
    request: &GenerationRequest,
) -> Result<(), GenerationError> {
    let snapshot = glance_core::snapshot_tree(&request.source_root, request.source_sha.clone())
        .map_err(|error| GenerationError::Context {
            message: error.to_string(),
        })?;
    let failures = glance_check::validate_navigation(html, &snapshot);
    if failures.is_empty() {
        return Ok(());
    }
    let summary = failures
        .iter()
        .map(|failure| format!("{}: {}", failure.directory.display(), failure.message))
        .collect::<Vec<_>>()
        .join("; ");
    Err(GenerationError::InvalidHtml {
        message: format!("navigation validation failed: {summary}"),
    })
}

pub struct FallbackClient {
    providers: Vec<Box<dyn ProviderClient>>,
    retry: RetryConfig,
}

impl FallbackClient {
    pub fn new(providers: Vec<Box<dyn ProviderClient>>, retry: RetryConfig) -> Self {
        Self { providers, retry }
    }
}

impl ProviderClient for FallbackClient {
    fn name(&self) -> &'static str {
        "fallback"
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::OpenRouter
    }

    fn generate_once(
        &self,
        prompt: &PromptContext,
        route: &ModelRoute,
    ) -> Result<ProviderOutput, GenerationError> {
        if self.providers.is_empty() {
            return Err(GenerationError::Provider {
                provider: "fallback",
                retryable: false,
                message: "no providers configured".to_owned(),
            });
        }

        let mut last_error = None;
        for attempt in 0..self.retry.max_attempts.max(1) {
            for provider in self.matching_providers(route.provider) {
                match provider.generate_once(prompt, route) {
                    Ok(output) => return Ok(output),
                    Err(error) if is_retryable(&error) => {
                        last_error = Some(error);
                    }
                    Err(error) => {
                        last_error = Some(error);
                        continue;
                    }
                }
            }
            if attempt + 1 < self.retry.max_attempts {
                let delay = retry_delay(&self.retry, attempt);
                if !delay.is_zero() {
                    thread::sleep(delay);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| GenerationError::Provider {
            provider: "fallback",
            retryable: false,
            message: "no provider attempts were made".to_owned(),
        }))
    }
}

impl FallbackClient {
    fn matching_providers(&self, preferred: ProviderKind) -> Vec<&dyn ProviderClient> {
        self.providers
            .iter()
            .filter(|provider| provider.kind() == preferred)
            .map(|provider| provider.as_ref())
            .collect()
    }
}

fn is_retryable(error: &GenerationError) -> bool {
    matches!(
        error,
        GenerationError::Provider {
            retryable: true,
            ..
        } | GenerationError::Http(_)
    )
}

fn retry_delay(config: &RetryConfig, attempt: usize) -> Duration {
    if config.base_backoff_millis == 0 && config.jitter_millis == 0 {
        return Duration::ZERO;
    }
    let exponent = 1_u64.checked_shl(attempt as u32).unwrap_or(u64::MAX);
    let base = config.base_backoff_millis.saturating_mul(exponent);
    let jitter = if config.jitter_millis == 0 {
        0
    } else {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| u64::from(duration.subsec_nanos()) % config.jitter_millis)
            .unwrap_or(0)
    };
    Duration::from_millis(base.saturating_add(jitter))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

pub trait HttpTransport: Send + Sync {
    fn post_json(
        &self,
        url: &str,
        headers: &[(&str, String)],
        body: &Value,
    ) -> Result<HttpResponse, GenerationError>;
}

pub struct UreqTransport;

impl HttpTransport for UreqTransport {
    fn post_json(
        &self,
        url: &str,
        headers: &[(&str, String)],
        body: &Value,
    ) -> Result<HttpResponse, GenerationError> {
        let mut request = ureq::post(url).set("Content-Type", "application/json");
        for (name, value) in headers {
            request = request.set(name, value);
        }

        match request.send_string(&body.to_string()) {
            Ok(response) => Ok(HttpResponse {
                status: response.status(),
                body: response
                    .into_string()
                    .map_err(|error| GenerationError::Http(error.to_string()))?,
            }),
            Err(ureq::Error::Status(status, response)) => Ok(HttpResponse {
                status,
                body: response
                    .into_string()
                    .unwrap_or_else(|_| "<unreadable response body>".to_owned()),
            }),
            Err(error) => Err(GenerationError::Http(error.to_string())),
        }
    }
}

pub struct OpenRouterClient {
    api_key: String,
    endpoint: String,
    transport: Arc<dyn HttpTransport>,
}

impl OpenRouterClient {
    pub fn from_env(transport: Arc<dyn HttpTransport>) -> Result<Self, GenerationError> {
        let api_key =
            std::env::var("OPENROUTER_API_KEY").map_err(|_| GenerationError::MissingSecret {
                name: "OPENROUTER_API_KEY",
            })?;
        Ok(Self {
            api_key,
            endpoint: "https://openrouter.ai/api/v1/chat/completions".to_owned(),
            transport,
        })
    }

    pub fn new(api_key: String, endpoint: String, transport: Arc<dyn HttpTransport>) -> Self {
        Self {
            api_key,
            endpoint,
            transport,
        }
    }
}

impl ProviderClient for OpenRouterClient {
    fn name(&self) -> &'static str {
        "openrouter"
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::OpenRouter
    }

    fn generate_once(
        &self,
        prompt: &PromptContext,
        route: &ModelRoute,
    ) -> Result<ProviderOutput, GenerationError> {
        let body = json!({
            "model": route.model,
            "messages": [
                {
                    "role": "system",
                    "content": "You generate cited, self-contained HTML pages for glance. Return only raw HTML; no prose or Markdown fences."
                },
                {
                    "role": "user",
                    "content": prompt.prompt
                }
            ],
            "max_completion_tokens": route.max_tokens,
            "stream": false
        });
        let headers = vec![("Authorization", format!("Bearer {}", self.api_key))];
        let response = self.transport.post_json(&self.endpoint, &headers, &body)?;
        if response.status >= 400 {
            return Err(provider_http_error(
                "openrouter",
                response.status,
                response.body,
            ));
        }

        let value = serde_json::from_str::<Value>(&response.body)
            .map_err(|error| GenerationError::InvalidProviderResponse(error.to_string()))?;
        let html = value
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                GenerationError::InvalidProviderResponse("choices[0].message.content".to_owned())
            })?
            .to_owned();
        let usage = &value["usage"];
        let input_tokens = usage["prompt_tokens"]
            .as_u64()
            .unwrap_or(prompt.estimated_input_tokens);
        let output_tokens = usage["completion_tokens"].as_u64().unwrap_or(0);
        let spend_micros = usage["cost"]
            .as_f64()
            .map(|cost| (cost * 1_000_000.0).ceil() as u64)
            .filter(|spend_micros| {
                *spend_micros > 0 || input_tokens.saturating_add(output_tokens) == 0
            });
        let finish_reason = value["choices"][0]["finish_reason"]
            .as_str()
            .map(str::to_owned);

        Ok(ProviderOutput {
            html,
            provider: self.name().to_owned(),
            model: value["model"].as_str().unwrap_or(&route.model).to_owned(),
            input_tokens,
            output_tokens,
            spend_micros,
            finish_reason,
        })
    }
}

pub struct GeminiClient {
    api_key: String,
    endpoint_base: String,
    transport: Arc<dyn HttpTransport>,
}

impl GeminiClient {
    pub fn from_env(transport: Arc<dyn HttpTransport>) -> Result<Self, GenerationError> {
        let api_key =
            std::env::var("GEMINI_API_KEY").map_err(|_| GenerationError::MissingSecret {
                name: "GEMINI_API_KEY",
            })?;
        Ok(Self {
            api_key,
            endpoint_base: "https://generativelanguage.googleapis.com/v1beta/models".to_owned(),
            transport,
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

impl ProviderClient for GeminiClient {
    fn name(&self) -> &'static str {
        "gemini"
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Gemini
    }

    fn generate_once(
        &self,
        prompt: &PromptContext,
        route: &ModelRoute,
    ) -> Result<ProviderOutput, GenerationError> {
        let model = route.model.trim_start_matches("models/");
        let endpoint = format!(
            "{}/{}:generateContent",
            self.endpoint_base.trim_end_matches('/'),
            path_segment_encode(model)
        );
        let body = json!({
            "contents": [
                {
                    "parts": [
                        { "text": prompt.prompt }
                    ]
                }
            ],
            "generationConfig": {
                "maxOutputTokens": route.max_tokens
            }
        });
        let headers = vec![("x-goog-api-key", self.api_key.clone())];
        let response = self.transport.post_json(&endpoint, &headers, &body)?;
        if response.status >= 400 {
            return Err(provider_http_error(
                "gemini",
                response.status,
                response.body,
            ));
        }

        let value = serde_json::from_str::<Value>(&response.body)
            .map_err(|error| GenerationError::InvalidProviderResponse(error.to_string()))?;
        let parts = value["candidates"][0]["content"]["parts"]
            .as_array()
            .ok_or_else(|| {
                GenerationError::InvalidProviderResponse("candidates[0].content.parts".to_owned())
            })?;
        let html = parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<String>();
        if html.is_empty() {
            return Err(GenerationError::InvalidProviderResponse(
                "candidate text".to_owned(),
            ));
        }
        let usage = &value["usageMetadata"];
        let input_tokens = usage["promptTokenCount"]
            .as_u64()
            .unwrap_or(prompt.estimated_input_tokens);
        let output_tokens = usage["candidatesTokenCount"].as_u64().unwrap_or(0);
        let finish_reason = value["candidates"][0]["finishReason"]
            .as_str()
            .map(str::to_owned);

        Ok(ProviderOutput {
            html,
            provider: self.name().to_owned(),
            model: route.model.clone(),
            input_tokens,
            output_tokens,
            spend_micros: None,
            finish_reason,
        })
    }
}

fn provider_http_error(provider: &'static str, status: u16, body: String) -> GenerationError {
    let retryable = matches!(status, 408 | 429 | 500..=599);
    let message = format!("http {status}: {}", sanitize_error_body(&body));
    GenerationError::Provider {
        provider,
        retryable,
        message,
    }
}

fn sanitize_error_body(body: &str) -> String {
    body.chars().take(500).collect()
}

fn path_segment_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

pub fn spend_report_lines(report: &SpendReport) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!(
        "spend_report pages={} input_tokens={} output_tokens={} spend_micros={}",
        report.total_pages, report.input_tokens, report.output_tokens, report.spend_micros
    ));
    if report.spend_micros == 0 && report.input_tokens.saturating_add(report.output_tokens) > 0 {
        lines.push(format!(
            "warning=zero_spend_with_tokens input_tokens={} output_tokens={}",
            report.input_tokens, report.output_tokens
        ));
    }
    for page in &report.pages {
        lines.push(format!(
            "spend_page={} provider={} model={} input_tokens={} output_tokens={} spend_micros={}",
            page.directory.display(),
            page.provider,
            page.model,
            page.input_tokens,
            page.output_tokens,
            page.spend_micros
        ));
        if page.spend_micros == 0 && page.input_tokens.saturating_add(page.output_tokens) > 0 {
            lines.push(format!(
                "warning_page={} warning=zero_spend_with_tokens input_tokens={} output_tokens={}",
                page.directory.display(),
                page.input_tokens,
                page.output_tokens
            ));
        }
    }
    lines
}

#[cfg(test)]
#[derive(Default)]
struct RecordingTransport {
    responses: std::sync::Mutex<Vec<HttpResponse>>,
    requests: std::sync::Mutex<Vec<RecordedRequest>>,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordedRequest {
    url: String,
    headers: BTreeMap<String, String>,
    body: Value,
}

#[cfg(test)]
impl RecordingTransport {
    fn with_response(response: HttpResponse) -> Self {
        Self {
            responses: std::sync::Mutex::new(vec![response]),
            requests: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[cfg(test)]
impl HttpTransport for RecordingTransport {
    fn post_json(
        &self,
        url: &str,
        headers: &[(&str, String)],
        body: &Value,
    ) -> Result<HttpResponse, GenerationError> {
        self.requests
            .lock()
            .expect("requests")
            .push(RecordedRequest {
                url: url.to_owned(),
                headers: headers
                    .iter()
                    .map(|(name, value)| ((*name).to_owned(), value.clone()))
                    .collect(),
                body: body.clone(),
            });
        self.responses
            .lock()
            .expect("responses")
            .pop()
            .ok_or_else(|| GenerationError::Http("no recorded response".to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    #[test]
    fn routes_page_kinds_to_model_tiers() {
        let router = TierRouter::default();
        assert_eq!(router.tier_for(PageKind::Leaf), ModelTier::Cheap);
        assert_eq!(router.tier_for(PageKind::Interior), ModelTier::Mid);
        assert_eq!(router.tier_for(PageKind::Root), ModelTier::Frontier);
    }

    #[test]
    fn depth_defaults_route_to_requested_model_slugs() {
        let routing = GenerationConfig::default().routing;

        assert_eq!(
            routing.model_for(PageKind::Leaf).model,
            "deepseek/deepseek-v4-flash"
        );
        assert_eq!(routing.model_for(PageKind::Leaf).max_tokens, 6_000);
        assert_eq!(
            routing.model_for(PageKind::Interior).model,
            "anthropic/claude-sonnet-5"
        );
        assert_eq!(routing.model_for(PageKind::Interior).max_tokens, 10_000);
        assert_eq!(routing.model_for(PageKind::Root).model, "openai/gpt-5.5");
        assert_eq!(routing.model_for(PageKind::Root).max_tokens, 16_000);
    }

    #[test]
    fn budget_preflight_fails_before_provider_call() {
        let mut budget = BudgetTracker::new(BudgetConfig {
            per_run_micros: Some(10),
            per_day_micros: Some(100),
            spent_today_micros: 0,
        });

        let error = budget.reserve(11).expect_err("budget must fail closed");

        assert!(matches!(error, GenerationError::BudgetExceeded { .. }));
        assert_eq!(budget.report().total_pages, 0);
    }

    #[test]
    fn prompt_context_truncates_on_utf8_boundary_and_records_note() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join("README.md"), "fixture repo").expect("readme");
        std::fs::create_dir(temp.path().join("leaf")).expect("leaf dir");
        std::fs::write(temp.path().join("leaf/data.txt"), "abc😄def").expect("fixture");

        let context =
            PromptContext::from_directory(temp.path(), Path::new("leaf"), 6).expect("context");

        assert!(context.prompt.contains("abc"));
        assert!(!context.prompt.contains("abc😄def"));
        assert!(
            context
                .metadata_notes
                .iter()
                .any(|note| note.contains("truncated leaf/data.txt"))
        );
    }

    #[test]
    fn openrouter_client_uses_chat_completion_wire_format() {
        let transport = Arc::new(RecordingTransport::with_response(HttpResponse {
            status: 200,
            body: json!({
                "model": "deepseek/deepseek-v4-flash",
                "choices": [{"message": {"content": "<!doctype html><p>ok</p>"}}],
                "usage": {"prompt_tokens": 11, "completion_tokens": 7, "total_tokens": 18, "cost": 0.000012}
            })
            .to_string(),
        }));
        let client = OpenRouterClient::new(
            "secret-key".to_owned(),
            "http://127.0.0.1/chat".to_owned(),
            transport.clone(),
        );
        let prompt = PromptContext {
            prompt: "source".to_owned(),
            prompt_version: "test-prompt".to_owned(),
            estimated_input_tokens: 2,
            metadata_notes: Vec::new(),
            primary_citation: None,
            degraded_children: Vec::new(),
        };

        let output = client
            .generate_once(
                &prompt,
                GenerationConfig::default()
                    .routing
                    .model_for(PageKind::Leaf),
            )
            .expect("provider output");

        assert_eq!(output.html, "<!doctype html><p>ok</p>");
        assert_eq!(output.input_tokens, 11);
        assert_eq!(output.output_tokens, 7);
        assert_eq!(output.spend_micros, Some(12));
        let requests = transport.requests.lock().expect("requests");
        let request = requests.first().expect("request");
        assert_eq!(request.url, "http://127.0.0.1/chat");
        assert_eq!(request.headers["Authorization"], "Bearer secret-key");
        assert_eq!(request.body["model"], "deepseek/deepseek-v4-flash");
        assert_eq!(request.body["max_completion_tokens"], 6_000);
        assert_eq!(request.body["stream"], false);
    }

    #[test]
    fn openrouter_client_works_against_local_mock_http_server() {
        let (endpoint, requests) = one_shot_server(
            200,
            json!({
                "model": "deepseek/deepseek-v4-flash",
                "choices": [{"message": {"content": "<!doctype html><p>server</p>"}}],
                "usage": {"prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5, "cost": 0.000001}
            })
            .to_string(),
        );
        let client =
            OpenRouterClient::new("server-key".to_owned(), endpoint, Arc::new(UreqTransport));
        let prompt = PromptContext {
            prompt: "wire prompt".to_owned(),
            prompt_version: "test-prompt".to_owned(),
            estimated_input_tokens: 3,
            metadata_notes: Vec::new(),
            primary_citation: None,
            degraded_children: Vec::new(),
        };

        let output = client
            .generate_once(
                &prompt,
                GenerationConfig::default()
                    .routing
                    .model_for(PageKind::Leaf),
            )
            .expect("server output");

        assert_eq!(output.html, "<!doctype html><p>server</p>");
        let request = requests.recv().expect("captured request");
        let request_lower = request.to_ascii_lowercase();
        assert!(request.contains("POST / HTTP/1.1"));
        assert!(request_lower.contains("authorization: bearer server-key"));
        assert!(request.contains("\"max_completion_tokens\":6000"));
        assert!(request.contains("\"model\":\"deepseek/deepseek-v4-flash\""));
    }

    #[test]
    fn gemini_client_uses_generate_content_wire_format() {
        let transport = Arc::new(RecordingTransport::with_response(HttpResponse {
            status: 200,
            body: json!({
                "candidates": [{
                    "content": {"parts": [{"text": "<!doctype html><p>gemini</p>"}]}
                }],
                "usageMetadata": {"promptTokenCount": 13, "candidatesTokenCount": 8, "totalTokenCount": 21}
            })
            .to_string(),
        }));
        let client = GeminiClient::new(
            "gemini-key".to_owned(),
            "http://127.0.0.1/v1beta/models".to_owned(),
            transport.clone(),
        );
        let prompt = PromptContext {
            prompt: "source".to_owned(),
            prompt_version: "test-prompt".to_owned(),
            estimated_input_tokens: 2,
            metadata_notes: Vec::new(),
            primary_citation: None,
            degraded_children: Vec::new(),
        };
        let route = ModelRoute {
            provider: ProviderKind::Gemini,
            model: "gemini-3.5-flash".to_owned(),
            ..GenerationConfig::default().routing.leaf
        };

        let output = client
            .generate_once(&prompt, &route)
            .expect("provider output");

        assert_eq!(output.html, "<!doctype html><p>gemini</p>");
        assert_eq!(output.input_tokens, 13);
        assert_eq!(output.output_tokens, 8);
        assert_eq!(output.spend_micros, None);
        let requests = transport.requests.lock().expect("requests");
        let request = requests.first().expect("request");
        assert_eq!(
            request.url,
            "http://127.0.0.1/v1beta/models/gemini-3.5-flash:generateContent"
        );
        assert_eq!(request.headers["x-goog-api-key"], "gemini-key");
        assert_eq!(request.body["generationConfig"]["maxOutputTokens"], 6_000);
        assert_eq!(request.body["contents"][0]["parts"][0]["text"], "source");
    }

    #[test]
    fn gemini_client_works_against_local_mock_http_server() {
        let (endpoint_base, requests) = one_shot_server(
            200,
            json!({
                "candidates": [{
                    "content": {"parts": [{"text": "<!doctype html><p>native</p>"}]}
                }],
                "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 4, "totalTokenCount": 9}
            })
            .to_string(),
        );
        let endpoint_base = format!("{endpoint_base}/v1beta/models");
        let client = GeminiClient::new(
            "native-key".to_owned(),
            endpoint_base,
            Arc::new(UreqTransport),
        );
        let prompt = PromptContext {
            prompt: "gemini prompt".to_owned(),
            prompt_version: "test-prompt".to_owned(),
            estimated_input_tokens: 3,
            metadata_notes: Vec::new(),
            primary_citation: None,
            degraded_children: Vec::new(),
        };
        let route = ModelRoute {
            provider: ProviderKind::Gemini,
            model: "gemini-3.5-flash".to_owned(),
            ..GenerationConfig::default().routing.leaf
        };

        let output = client
            .generate_once(&prompt, &route)
            .expect("server output");

        assert_eq!(output.html, "<!doctype html><p>native</p>");
        let request = requests.recv().expect("captured request");
        let request_lower = request.to_ascii_lowercase();
        assert!(request.contains("POST /v1beta/models/gemini-3.5-flash:generateContent HTTP/1.1"));
        assert!(request_lower.contains("x-goog-api-key: native-key"));
        assert!(request.contains("\"maxOutputTokens\":6000"));
        assert!(request.contains("\"text\":\"gemini prompt\""));
    }

    #[test]
    fn fallback_client_owns_retries_while_inner_clients_are_single_attempt() {
        let first = Box::new(CountingClient {
            name: "first",
            kind: ProviderKind::OpenRouter,
            attempts: Arc::new(AtomicUsize::new(0)),
            fail_until: usize::MAX,
        });
        let first_attempts = first.attempts.clone();
        let second = Box::new(CountingClient {
            name: "second",
            kind: ProviderKind::OpenRouter,
            attempts: Arc::new(AtomicUsize::new(0)),
            fail_until: 1,
        });
        let second_attempts = second.attempts.clone();
        let client = FallbackClient::new(
            vec![first, second],
            RetryConfig {
                max_attempts: 2,
                base_backoff_millis: 0,
                jitter_millis: 0,
            },
        );
        let prompt = PromptContext {
            prompt: "source".to_owned(),
            prompt_version: "test-prompt".to_owned(),
            estimated_input_tokens: 2,
            metadata_notes: Vec::new(),
            primary_citation: None,
            degraded_children: Vec::new(),
        };

        let output = client
            .generate_once(
                &prompt,
                GenerationConfig::default()
                    .routing
                    .model_for(PageKind::Leaf),
            )
            .expect("fallback output");

        assert_eq!(output.provider, "second");
        assert_eq!(first_attempts.load(Ordering::SeqCst), 2);
        assert_eq!(second_attempts.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn fallback_client_does_not_cross_provider_with_provider_specific_model_slug() {
        let openrouter = Box::new(CountingClient {
            name: "openrouter",
            kind: ProviderKind::OpenRouter,
            attempts: Arc::new(AtomicUsize::new(0)),
            fail_until: 0,
        });
        let openrouter_attempts = openrouter.attempts.clone();
        let gemini = Box::new(CountingClient {
            name: "gemini",
            kind: ProviderKind::Gemini,
            attempts: Arc::new(AtomicUsize::new(0)),
            fail_until: usize::MAX,
        });
        let gemini_attempts = gemini.attempts.clone();
        let client = FallbackClient::new(
            vec![openrouter, gemini],
            RetryConfig {
                max_attempts: 1,
                base_backoff_millis: 0,
                jitter_millis: 0,
            },
        );
        let prompt = PromptContext {
            prompt: "source".to_owned(),
            prompt_version: "test-prompt".to_owned(),
            estimated_input_tokens: 2,
            metadata_notes: Vec::new(),
            primary_citation: None,
            degraded_children: Vec::new(),
        };
        let route = ModelRoute {
            provider: ProviderKind::Gemini,
            model: "gemini-3.5-flash".to_owned(),
            ..GenerationConfig::default().routing.leaf
        };

        let error = client
            .generate_once(&prompt, &route)
            .expect_err("preferred provider error");

        assert!(matches!(
            error,
            GenerationError::Provider {
                provider: "gemini",
                retryable: true,
                ..
            }
        ));
        assert_eq!(gemini_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(openrouter_attempts.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn real_generator_enforces_budget_before_calling_provider() {
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = Box::new(BudgetCountingClient {
            calls: calls.clone(),
        });
        let config = GenerationConfig {
            budget: BudgetConfig {
                per_run_micros: Some(1),
                per_day_micros: Some(1),
                spent_today_micros: 0,
            },
            ..GenerationConfig::default()
        };
        let generator = RealPageGenerator::new(config, vec![provider]);
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join("README.md"), "hello").expect("fixture");

        let error = generator
            .generate(GenerationRequest::new(
                temp.path().to_path_buf(),
                PathBuf::from("."),
                "sha".to_owned(),
                PageKind::Leaf,
            ))
            .expect_err("budget error");

        assert!(matches!(error, GenerationError::BudgetExceeded { .. }));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn real_generator_rejects_provider_length_truncation() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let provider = Box::new(ScriptedClient {
            name: "scripted",
            attempts: attempts.clone(),
            prompts: Arc::new(std::sync::Mutex::new(Vec::new())),
            outputs: std::sync::Mutex::new(vec![Ok(provider_output(
                "<!doctype html><html><body><p>cut",
                Some("length"),
                Some(1),
            ))]),
        });
        let generator = RealPageGenerator::new(generation_config_for_validation(), vec![provider]);
        let request = request_with_prompt(PageKind::Root);

        let error = generator
            .generate(request)
            .expect_err("length truncation must fail");

        assert!(matches!(error, GenerationError::InvalidHtml { .. }));
        assert!(error.to_string().contains("finish_reason=length"));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert_eq!(generator.spend_report().total_pages, 0);
    }

    #[test]
    fn real_generator_rejects_missing_html_close_as_truncation() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let provider = Box::new(ScriptedClient {
            name: "scripted",
            attempts: attempts.clone(),
            prompts: Arc::new(std::sync::Mutex::new(Vec::new())),
            outputs: std::sync::Mutex::new(vec![Ok(provider_output(
                "<!doctype html><html><body><p>cut",
                Some("stop"),
                Some(1),
            ))]),
        });
        let generator = RealPageGenerator::new(generation_config_for_validation(), vec![provider]);

        let error = generator
            .generate(request_with_prompt(PageKind::Leaf))
            .expect_err("missing closing html must fail");

        assert!(matches!(error, GenerationError::InvalidHtml { .. }));
        assert!(error.to_string().contains("missing closing </html>"));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert_eq!(generator.spend_report().total_pages, 0);
    }

    #[test]
    fn real_generator_retries_bad_citation_once_then_fails_loudly() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let prompts = Arc::new(std::sync::Mutex::new(Vec::new()));
        let bad_html =
            r#"<!doctype html><html><body><p data-glance-cite="21-26">bad cite</p></body></html>"#;
        let provider = Box::new(ScriptedClient {
            name: "scripted",
            attempts: attempts.clone(),
            prompts: prompts.clone(),
            outputs: std::sync::Mutex::new(vec![
                Ok(provider_output(bad_html, Some("stop"), Some(1))),
                Ok(provider_output(bad_html, Some("stop"), Some(1))),
            ]),
        });
        let generator = RealPageGenerator::new(generation_config_for_validation(), vec![provider]);

        let error = generator
            .generate(request_with_prompt(PageKind::Leaf))
            .expect_err("bad citation must fail after retry");

        assert!(matches!(error, GenerationError::InvalidHtml { .. }));
        assert!(error.to_string().contains("data-glance-cite"));
        assert!(error.to_string().contains("path:start[-end]"));
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        let prompts = prompts.lock().expect("prompts");
        assert_eq!(prompts.len(), 2);
        assert!(prompts[1].contains("Previous output was rejected"));
        assert!(prompts[1].contains("data-glance-cite"));
        assert_eq!(generator.spend_report().total_pages, 0);
    }

    #[test]
    fn real_generator_retries_missing_navigation_then_accepts_fixed_page() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join("README.md"), "one\n").expect("fixture");
        let attempts = Arc::new(AtomicUsize::new(0));
        let prompts = Arc::new(std::sync::Mutex::new(Vec::new()));
        let bad_html = r#"<!doctype html><html><body><p data-glance-cite="README.md:1">missing nav</p></body></html>"#;
        let good_html = r#"<!doctype html><html><body class="glance-page" data-glance-directory="."><nav class="glance-nav"></nav><p data-glance-cite="README.md:1">fixed nav</p></body></html>"#;
        let provider = Box::new(ScriptedClient {
            name: "scripted",
            attempts: attempts.clone(),
            prompts: prompts.clone(),
            outputs: std::sync::Mutex::new(vec![
                Ok(provider_output(good_html, Some("stop"), Some(1))),
                Ok(provider_output(bad_html, Some("stop"), Some(1))),
            ]),
        });
        let generator = RealPageGenerator::new(generation_config_for_validation(), vec![provider]);

        let page = generator
            .generate(
                GenerationRequest::new(
                    temp.path().to_path_buf(),
                    PathBuf::from("."),
                    "sha".to_owned(),
                    PageKind::Root,
                )
                .with_prompt_context(PromptContext {
                    prompt: "source prompt".to_owned(),
                    prompt_version: "test-prompt".to_owned(),
                    estimated_input_tokens: 1,
                    metadata_notes: Vec::new(),
                    primary_citation: None,
                    degraded_children: Vec::new(),
                }),
            )
            .expect("navigation retry should recover");

        assert!(page.html.contains("data-glance-directory"));
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert!(
            prompts
                .lock()
                .expect("prompts")
                .last()
                .expect("retry prompt")
                .contains("navigation validation failed")
        );
    }

    #[test]
    fn real_generator_fills_empty_navigation_directory_placeholder() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join("README.md"), "one\n").expect("fixture");
        let attempts = Arc::new(AtomicUsize::new(0));
        let html = r#"<!doctype html><html><body class="glance-page" data-glance-directory="   "><nav class="glance-nav"></nav><p data-glance-cite="README.md:1">root page</p></body></html>"#;
        let provider = Box::new(ScriptedClient {
            name: "scripted",
            attempts: attempts.clone(),
            prompts: Arc::new(std::sync::Mutex::new(Vec::new())),
            outputs: std::sync::Mutex::new(vec![Ok(provider_output(html, Some("stop"), Some(1)))]),
        });
        let generator = RealPageGenerator::new(generation_config_for_validation(), vec![provider]);

        let page = generator
            .generate(
                GenerationRequest::new(
                    temp.path().to_path_buf(),
                    PathBuf::from("."),
                    "sha".to_owned(),
                    PageKind::Root,
                )
                .with_prompt_context(PromptContext {
                    prompt: "source prompt".to_owned(),
                    prompt_version: "test-prompt".to_owned(),
                    estimated_input_tokens: 1,
                    metadata_notes: Vec::new(),
                    primary_citation: None,
                    degraded_children: Vec::new(),
                }),
            )
            .expect("empty directory placeholder should be filled");

        assert!(page.html.contains(r#"data-glance-directory=".""#));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn normalize_glance_directory_attribute_repairs_model_coordinates() {
        let html = r#"<!doctype html><html><body class="glance-page" data-glance-directory><nav></nav></body></html>"#;
        let wrong = r#"<!doctype html><html><body class="glance-page" data-glance-directory="."><nav></nav></body></html>"#;

        let repaired = normalize_glance_directory_attribute(html, Path::new("src/parser"));
        let corrected = normalize_glance_directory_attribute(wrong, Path::new("src/parser"));

        assert!(repaired.contains(r#"data-glance-directory="src/parser">"#));
        assert!(corrected.contains(r#"data-glance-directory="src/parser">"#));
    }

    #[test]
    fn real_generator_injects_required_parent_navigation_link() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(temp.path().join("src")).expect("src");
        std::fs::write(temp.path().join("src/lib.rs"), "pub fn demo() {}\n").expect("fixture");
        let attempts = Arc::new(AtomicUsize::new(0));
        let html = r#"<!doctype html><html><body class="glance-page" data-glance-directory="src"><p data-glance-cite="lib.rs:1">source page</p></body></html>"#;
        let provider = Box::new(ScriptedClient {
            name: "scripted",
            attempts: attempts.clone(),
            prompts: Arc::new(std::sync::Mutex::new(Vec::new())),
            outputs: std::sync::Mutex::new(vec![Ok(provider_output(html, Some("stop"), Some(1)))]),
        });
        let generator = RealPageGenerator::new(generation_config_for_validation(), vec![provider]);

        let page = generator
            .generate(
                GenerationRequest::new(
                    temp.path().to_path_buf(),
                    PathBuf::from("src"),
                    "sha".to_owned(),
                    PageKind::Leaf,
                )
                .with_prompt_context(PromptContext {
                    prompt: "source prompt".to_owned(),
                    prompt_version: "test-prompt".to_owned(),
                    estimated_input_tokens: 1,
                    metadata_notes: Vec::new(),
                    primary_citation: None,
                    degraded_children: Vec::new(),
                }),
            )
            .expect("parent link should be injected");

        assert!(
            page.html
                .contains(r#"class="glance-nav glance-nav-generated""#)
        );
        assert!(page.html.contains(r#"href="../index.html""#));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn real_generator_injects_root_flow_diagram_when_missing() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(temp.path().join("src")).expect("src");
        std::fs::write(temp.path().join("README.md"), "root\n").expect("readme");
        std::fs::write(temp.path().join("src/lib.rs"), "pub fn demo() {}\n").expect("fixture");
        let html = r#"<!doctype html><html><body class="glance-page" data-glance-directory="."><p data-glance-cite="README.md:1">root page</p></body></html>"#;
        let provider = Box::new(ScriptedClient {
            name: "scripted",
            attempts: Arc::new(AtomicUsize::new(0)),
            prompts: Arc::new(std::sync::Mutex::new(Vec::new())),
            outputs: std::sync::Mutex::new(vec![Ok(provider_output(html, Some("stop"), Some(1)))]),
        });
        let generator = RealPageGenerator::new(generation_config_for_validation(), vec![provider]);

        let page = generator
            .generate(
                GenerationRequest::new(
                    temp.path().to_path_buf(),
                    PathBuf::from("."),
                    "sha".to_owned(),
                    PageKind::Root,
                )
                .with_prompt_context(PromptContext {
                    prompt: "source prompt".to_owned(),
                    prompt_version: "test-prompt".to_owned(),
                    estimated_input_tokens: 1,
                    metadata_notes: Vec::new(),
                    primary_citation: None,
                    degraded_children: Vec::new(),
                }),
            )
            .expect("root diagram should be injected");

        assert!(page.html.contains("glance-diagram"));
        assert!(page.html.contains("@keyframes glance-flow-pulse"));
        assert!(page.html.contains("src"));
    }

    #[test]
    fn real_generator_accepts_one_path_with_multiple_citation_ranges() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(temp.path().join("src")).expect("src");
        std::fs::write(temp.path().join("src/lib.rs"), "one\ntwo\nthree\n").expect("fixture");
        let attempts = Arc::new(AtomicUsize::new(0));
        let html = r#"<!doctype html><html><body class="glance-page" data-glance-directory="."><nav class="glance-nav"><a href="src/index.html">src</a></nav><p data-glance-cite="src/lib.rs:1-2,3-3">split cite</p></body></html>"#;
        let provider = Box::new(ScriptedClient {
            name: "scripted",
            attempts: attempts.clone(),
            prompts: Arc::new(std::sync::Mutex::new(Vec::new())),
            outputs: std::sync::Mutex::new(vec![Ok(provider_output(html, Some("stop"), Some(1)))]),
        });
        let generator = RealPageGenerator::new(generation_config_for_validation(), vec![provider]);

        let page = generator
            .generate(
                GenerationRequest::new(
                    temp.path().to_path_buf(),
                    PathBuf::from("."),
                    "sha".to_owned(),
                    PageKind::Root,
                )
                .with_prompt_context(PromptContext {
                    prompt: "source prompt".to_owned(),
                    prompt_version: "test-prompt".to_owned(),
                    estimated_input_tokens: 1,
                    metadata_notes: Vec::new(),
                    primary_citation: None,
                    degraded_children: Vec::new(),
                }),
            )
            .expect("multi-range citation should be accepted");

        assert!(
            page.html
                .contains(r#"data-glance-cite="src/lib.rs:1-2,3-3""#)
        );
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn real_generator_accepts_multi_path_citation_attributes() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(temp.path().join("prompts")).expect("prompts");
        std::fs::write(temp.path().join("prompts/root.md"), "one\ntwo\nthree\n").expect("root");
        std::fs::write(temp.path().join("prompts/leaf.md"), "one\ntwo\nthree\n").expect("leaf");
        let attempts = Arc::new(AtomicUsize::new(0));
        let html = r#"<!doctype html><html><body class="glance-page" data-glance-directory="."><nav class="glance-nav"><a href="prompts/index.html">prompts</a></nav><p data-glance-cite="prompts/root.md:1-2,3-3,prompts/leaf.md:2-3">split paths</p></body></html>"#;
        let provider = Box::new(ScriptedClient {
            name: "scripted",
            attempts: attempts.clone(),
            prompts: Arc::new(std::sync::Mutex::new(Vec::new())),
            outputs: std::sync::Mutex::new(vec![Ok(provider_output(html, Some("stop"), Some(1)))]),
        });
        let generator = RealPageGenerator::new(generation_config_for_validation(), vec![provider]);

        let page = generator
            .generate(
                GenerationRequest::new(
                    temp.path().to_path_buf(),
                    PathBuf::from("."),
                    "sha".to_owned(),
                    PageKind::Root,
                )
                .with_prompt_context(PromptContext {
                    prompt: "source prompt".to_owned(),
                    prompt_version: "test-prompt".to_owned(),
                    estimated_input_tokens: 1,
                    metadata_notes: Vec::new(),
                    primary_citation: None,
                    degraded_children: Vec::new(),
                }),
            )
            .expect("multi-path citation should be accepted");

        assert!(
            page.html
                .contains(r#"data-glance-cite="prompts/root.md:1-2,3-3,prompts/leaf.md:2-3""#)
        );
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn real_generator_normalizes_dir_relative_citation_paths() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(temp.path().join("src")).expect("src");
        std::fs::write(temp.path().join("src/lib.rs"), "one\n").expect("fixture");
        let attempts = Arc::new(AtomicUsize::new(0));
        let html = r#"<!doctype html><html><body class="glance-page" data-glance-directory="src"><nav class="glance-nav"><a href="../index.html">Parent</a></nav><p data-glance-cite="lib.rs:1">dir-relative cite</p></body></html>"#;
        let provider = Box::new(ScriptedClient {
            name: "scripted",
            attempts: attempts.clone(),
            prompts: Arc::new(std::sync::Mutex::new(Vec::new())),
            outputs: std::sync::Mutex::new(vec![Ok(provider_output(html, Some("stop"), Some(1)))]),
        });
        let generator = RealPageGenerator::new(generation_config_for_validation(), vec![provider]);

        let page = generator
            .generate(
                GenerationRequest::new(
                    temp.path().to_path_buf(),
                    PathBuf::from("src"),
                    "sha".to_owned(),
                    PageKind::Leaf,
                )
                .with_prompt_context(PromptContext {
                    prompt: "source prompt".to_owned(),
                    prompt_version: "test-prompt".to_owned(),
                    estimated_input_tokens: 1,
                    metadata_notes: Vec::new(),
                    primary_citation: None,
                    degraded_children: Vec::new(),
                }),
            )
            .expect("dir-relative citation should normalize");

        assert!(page.html.contains(r#"data-glance-cite="src/lib.rs:1""#));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn real_generator_retries_unresolvable_citation_paths_then_fails() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(temp.path().join("src")).expect("src");
        std::fs::write(temp.path().join("src/lib.rs"), "one\n").expect("fixture");
        let attempts = Arc::new(AtomicUsize::new(0));
        let prompts = Arc::new(std::sync::Mutex::new(Vec::new()));
        let html = r#"<!doctype html><html><body><p data-glance-cite="missing.rs:1">missing cite</p></body></html>"#;
        let provider = Box::new(ScriptedClient {
            name: "scripted",
            attempts: attempts.clone(),
            prompts: prompts.clone(),
            outputs: std::sync::Mutex::new(vec![
                Ok(provider_output(html, Some("stop"), Some(1))),
                Ok(provider_output(html, Some("stop"), Some(1))),
            ]),
        });
        let generator = RealPageGenerator::new(generation_config_for_validation(), vec![provider]);

        let error = generator
            .generate(
                GenerationRequest::new(
                    temp.path().to_path_buf(),
                    PathBuf::from("src"),
                    "sha".to_owned(),
                    PageKind::Leaf,
                )
                .with_prompt_context(PromptContext {
                    prompt: "source prompt".to_owned(),
                    prompt_version: "test-prompt".to_owned(),
                    estimated_input_tokens: 1,
                    metadata_notes: Vec::new(),
                    primary_citation: None,
                    degraded_children: Vec::new(),
                }),
            )
            .expect_err("unresolvable citation must fail after retry");

        assert!(matches!(error, GenerationError::InvalidHtml { .. }));
        assert!(
            error
                .to_string()
                .contains("unresolvable data-glance-cite path")
        );
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert!(
            prompts
                .lock()
                .expect("prompts")
                .last()
                .expect("retry prompt")
                .contains("Previous output was rejected")
        );
    }

    #[test]
    fn spend_report_warns_when_tokens_have_zero_spend() {
        let mut report = SpendReport::default();
        report.record(PageSpend {
            directory: PathBuf::from("."),
            provider: "openrouter".to_owned(),
            model: "openai/gpt-5.5".to_owned(),
            input_tokens: 50_000,
            output_tokens: 0,
            spend_micros: 0,
        });

        let lines = spend_report_lines(&report);

        assert!(lines.iter().any(|line| {
            line.contains("warning=zero_spend_with_tokens input_tokens=50000 output_tokens=0")
        }));
        assert!(lines.iter().any(|line| {
            line.contains("warning_page=.") && line.contains("zero_spend_with_tokens")
        }));
    }

    #[test]
    fn live_smoke_generates_one_leaf_page_when_enabled() {
        if std::env::var("GLANCE_LIVE_SMOKE").ok().as_deref() != Some("1") {
            eprintln!("skipping live smoke; set GLANCE_LIVE_SMOKE=1");
            return;
        }

        let root = PathBuf::from("crates/glance-core/tests/fixtures/mini-source");
        let root = if root.exists() {
            root
        } else {
            PathBuf::from("../glance-core/tests/fixtures/mini-source")
        };
        let mut config = GenerationConfig {
            provider_mode: ProviderMode::Real,
            ..GenerationConfig::default()
        };
        if std::env::var("OPENROUTER_API_KEY").is_err() && std::env::var("GEMINI_API_KEY").is_ok() {
            config.routing.leaf.provider = ProviderKind::Gemini;
            config.routing.leaf.model = "gemini-3.5-flash".to_owned();
            config.routing.leaf.input_micros_per_million_tokens = 100;
            config.routing.leaf.output_micros_per_million_tokens = 400;
        }
        config.budget.per_run_micros = Some(50_000);
        config.budget.per_day_micros = Some(50_000);
        config.prompt.max_file_bytes = 4096;
        let generator = RealPageGenerator::from_env(config).expect("real generator");

        let page = generator
            .generate(GenerationRequest::new(
                root.clone(),
                PathBuf::from("src/parser"),
                "live-smoke".to_owned(),
                PageKind::Leaf,
            ))
            .expect("live page");

        println!(
            "live_smoke provider={} model={} input_tokens={} output_tokens={} spend_micros={} html_prefix={}",
            page.provider,
            page.model,
            page.input_tokens,
            page.output_tokens,
            page.spend_micros,
            page.html.chars().take(80).collect::<String>()
        );
        let normalized = page.html.trim_start().to_ascii_lowercase();
        assert!(normalized.starts_with("<!doctype html") || normalized.starts_with("<html"));
        assert!(page.input_tokens > 0);
    }

    struct CountingClient {
        name: &'static str,
        kind: ProviderKind,
        attempts: Arc<AtomicUsize>,
        fail_until: usize,
    }

    impl ProviderClient for CountingClient {
        fn name(&self) -> &'static str {
            self.name
        }

        fn kind(&self) -> ProviderKind {
            self.kind
        }

        fn generate_once(
            &self,
            _prompt: &PromptContext,
            _route: &ModelRoute,
        ) -> Result<ProviderOutput, GenerationError> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
            if attempt <= self.fail_until {
                return Err(GenerationError::Provider {
                    provider: self.name,
                    retryable: true,
                    message: "retry me".to_owned(),
                });
            }
            Ok(ProviderOutput {
                html: "<!doctype html><p>ok</p>".to_owned(),
                provider: self.name.to_owned(),
                model: "test-model".to_owned(),
                input_tokens: 1,
                output_tokens: 1,
                spend_micros: Some(1),
                finish_reason: Some("stop".to_owned()),
            })
        }
    }

    struct BudgetCountingClient {
        calls: Arc<AtomicUsize>,
    }

    impl ProviderClient for BudgetCountingClient {
        fn name(&self) -> &'static str {
            "budget-counting"
        }

        fn kind(&self) -> ProviderKind {
            ProviderKind::OpenRouter
        }

        fn generate_once(
            &self,
            _prompt: &PromptContext,
            _route: &ModelRoute,
        ) -> Result<ProviderOutput, GenerationError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(ProviderOutput {
                html: "<!doctype html><p>ok</p>".to_owned(),
                provider: self.name().to_owned(),
                model: "test-model".to_owned(),
                input_tokens: 1,
                output_tokens: 1,
                spend_micros: Some(1),
                finish_reason: Some("stop".to_owned()),
            })
        }
    }

    struct ScriptedClient {
        name: &'static str,
        attempts: Arc<AtomicUsize>,
        prompts: Arc<std::sync::Mutex<Vec<String>>>,
        outputs: std::sync::Mutex<Vec<Result<ProviderOutput, GenerationError>>>,
    }

    impl ProviderClient for ScriptedClient {
        fn name(&self) -> &'static str {
            self.name
        }

        fn kind(&self) -> ProviderKind {
            ProviderKind::OpenRouter
        }

        fn generate_once(
            &self,
            prompt: &PromptContext,
            _route: &ModelRoute,
        ) -> Result<ProviderOutput, GenerationError> {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            self.prompts
                .lock()
                .expect("prompts")
                .push(prompt.prompt.clone());
            self.outputs
                .lock()
                .expect("outputs")
                .pop()
                .expect("scripted output")
        }
    }

    fn generation_config_for_validation() -> GenerationConfig {
        GenerationConfig {
            retry: RetryConfig {
                max_attempts: 1,
                base_backoff_millis: 0,
                jitter_millis: 0,
            },
            budget: BudgetConfig {
                per_run_micros: Some(1_000_000),
                per_day_micros: Some(1_000_000),
                spent_today_micros: 0,
            },
            ..GenerationConfig::default()
        }
    }

    fn request_with_prompt(kind: PageKind) -> GenerationRequest {
        GenerationRequest::new(
            PathBuf::from("."),
            PathBuf::from("."),
            "sha".to_owned(),
            kind,
        )
        .with_prompt_context(PromptContext {
            prompt: "source prompt".to_owned(),
            prompt_version: "test-prompt".to_owned(),
            estimated_input_tokens: 1,
            metadata_notes: Vec::new(),
            primary_citation: None,
            degraded_children: Vec::new(),
        })
    }

    fn provider_output(
        html: &str,
        finish_reason: Option<&str>,
        spend_micros: Option<u64>,
    ) -> ProviderOutput {
        ProviderOutput {
            html: html.to_owned(),
            provider: "scripted".to_owned(),
            model: "test-model".to_owned(),
            input_tokens: 1,
            output_tokens: 1,
            spend_micros,
            finish_reason: finish_reason.map(str::to_owned),
        }
    }

    fn one_shot_server(status: u16, body: String) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let address = listener.local_addr().expect("address");
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buffer = Vec::new();
            let mut temp = [0_u8; 1024];
            let mut header_end = None;
            while header_end.is_none() {
                let read = stream.read(&mut temp).expect("read");
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&temp[..read]);
                header_end = find_header_end(&buffer);
            }
            if let Some(end) = header_end {
                let headers = String::from_utf8_lossy(&buffer[..end]).to_ascii_lowercase();
                let content_length = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                while buffer.len() < end + 4 + content_length {
                    let read = stream.read(&mut temp).expect("read body");
                    if read == 0 {
                        break;
                    }
                    buffer.extend_from_slice(&temp[..read]);
                }
            }
            sender
                .send(String::from_utf8_lossy(&buffer).to_string())
                .expect("send request");
            let response = format!(
                "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).expect("response");
        });
        (format!("http://{address}"), receiver)
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }
}

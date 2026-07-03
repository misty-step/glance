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
use context::validate_raw_html;
pub use context::{PromptContext, assemble_prompt_context};

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
                max_tokens: 900,
                input_micros_per_million_tokens: 40,
                output_micros_per_million_tokens: 180,
            },
            interior: ModelRoute {
                tier: ModelTier::Mid,
                provider: ProviderKind::OpenRouter,
                model: "anthropic/claude-sonnet-5".to_owned(),
                max_tokens: 1800,
                input_micros_per_million_tokens: 3_000,
                output_micros_per_million_tokens: 15_000,
            },
            root: ModelRoute {
                tier: ModelTier::Frontier,
                provider: ProviderKind::OpenRouter,
                model: "openai/gpt-5.5".to_owned(),
                max_tokens: 2600,
                input_micros_per_million_tokens: 10_000,
                output_micros_per_million_tokens: 30_000,
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
                let mut budget = self.budget.lock().expect("budget mutex");
                budget.reserved_micros = budget.reserved_micros.saturating_sub(estimated_micros);
                return Err(error);
            }
        };
        validate_raw_html(&output.html)?;

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
        })
    }
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
            .map(|cost| (cost * 1_000_000.0).ceil() as u64);

        Ok(ProviderOutput {
            html,
            provider: self.name().to_owned(),
            model: value["model"].as_str().unwrap_or(&route.model).to_owned(),
            input_tokens,
            output_tokens,
            spend_micros,
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

        Ok(ProviderOutput {
            html,
            provider: self.name().to_owned(),
            model: route.model.clone(),
            input_tokens,
            output_tokens,
            spend_micros: None,
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
        assert_eq!(
            routing.model_for(PageKind::Interior).model,
            "anthropic/claude-sonnet-5"
        );
        assert_eq!(routing.model_for(PageKind::Root).model, "openai/gpt-5.5");
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
        assert!(!context.prompt.contains("def"));
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
        assert_eq!(request.body["max_completion_tokens"], 900);
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
        assert!(request.contains("\"max_completion_tokens\":900"));
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
        assert_eq!(request.body["generationConfig"]["maxOutputTokens"], 900);
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
        assert!(request.contains("\"maxOutputTokens\":900"));
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
            })
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

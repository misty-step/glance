use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use glance_check::{Citation, CitationChecker, validate_navigation, validate_page_contract};
use glance_core::{RegenerationPlan, SourcePin, leaf_to_root_dirs, snapshot_tree};
use glance_gen::{
    ContextBlockMetadata, GeminiImageProvider, GeneratedPage, GenerationConfig, GenerationRequest,
    ImageBudget, ImageOutput, ImageProvider, ImageProviderKind, ImageRenderReport, ImageRequest,
    MockImageProvider, MockProvider, PageGenerator, PageKind, PageSpend, ProviderMode,
    RealPageGenerator, SpendReport, assemble_prompt_context_with_degraded,
    normalize_generated_html_citations, render_image_requests, spend_report_lines,
};
use glance_publish::{GhSisterHost, PublishRequest, SourceRepo};
use serde::{Deserialize, Serialize};
use serde_json::json;

mod canary;

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Generate and check citation-backed glance sites"
)]
struct Cli {
    #[arg(long, default_value = "glance.toml")]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run {
        #[arg(long)]
        root: Option<PathBuf>,
        #[arg(long)]
        site_root: Option<PathBuf>,
    },
    Plan {
        #[arg(long)]
        root: Option<PathBuf>,
        #[arg(long = "changed")]
        changed_paths: Vec<PathBuf>,
    },
    Check {
        #[arg(long)]
        source_root: Option<PathBuf>,
        #[arg(long)]
        source_sha: Option<String>,
        html: Vec<PathBuf>,
    },
    ServeLocal {
        #[arg(long)]
        site_root: Option<PathBuf>,
        #[arg(long, default_value_t = 4173)]
        port: u16,
        #[arg(long)]
        once: bool,
    },
    Publish {
        #[arg(long)]
        site_dir: PathBuf,
        #[arg(long)]
        source_owner: String,
        #[arg(long)]
        source_name: String,
        #[arg(long)]
        source_sha: String,
        #[arg(long, value_enum)]
        mode: PublishModeArg,
        #[arg(long)]
        sister_worktree: Option<PathBuf>,
        #[arg(long)]
        sister_remote: Option<String>,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long)]
        source_pr_title: Option<String>,
        #[arg(long)]
        run_id: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PublishModeArg {
    Branch,
    Master,
}

#[derive(Debug, Default, Deserialize)]
struct GlanceConfig {
    source_root: Option<PathBuf>,
    site_root: Option<PathBuf>,
    source_sha: Option<String>,
    changed_paths: Option<Vec<PathBuf>>,
    #[serde(default)]
    generation: GenerationConfig,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    canary::check_in();

    let result = run(cli);

    if let Err(error) = &result {
        canary::report_error("glance-next.generate.failed", &error.to_string());
    }
    canary::flush();

    result
}

fn run(cli: Cli) -> Result<()> {
    let config = load_config(&cli.config)?;

    match cli.command {
        Command::Run { root, site_root } => run_command(&config, root, site_root),
        Command::Plan {
            root,
            changed_paths,
        } => plan_command(&config, root, changed_paths),
        Command::Check {
            source_root,
            source_sha,
            html,
        } => check_command(&config, source_root, source_sha, html),
        Command::ServeLocal {
            site_root,
            port,
            once,
        } => serve_local_command(&config, site_root, port, once),
        Command::Publish {
            site_dir,
            source_owner,
            source_name,
            source_sha,
            mode,
            sister_worktree,
            sister_remote,
            branch,
            source_pr_title,
            run_id,
        } => publish_command(PublishCommand {
            site_dir,
            source_owner,
            source_name,
            source_sha,
            mode,
            sister_worktree,
            sister_remote,
            branch,
            source_pr_title,
            run_id,
        }),
    }
}

#[derive(Debug)]
struct PublishCommand {
    site_dir: PathBuf,
    source_owner: String,
    source_name: String,
    source_sha: String,
    mode: PublishModeArg,
    sister_worktree: Option<PathBuf>,
    sister_remote: Option<String>,
    branch: Option<String>,
    source_pr_title: Option<String>,
    run_id: Option<String>,
}

fn run_command(
    config: &GlanceConfig,
    root: Option<PathBuf>,
    site_root: Option<PathBuf>,
) -> Result<()> {
    let root = root
        .or_else(|| config.source_root.clone())
        .unwrap_or_else(|| PathBuf::from("."));
    let source_sha = configured_or_git_sha(config, &root)?;
    let snapshot = snapshot_tree(&root, &source_sha)?;
    let site_root = site_root
        .or_else(|| config.site_root.clone())
        .map(|site_root| resolve_site_root_outside_source(&site_root, &snapshot.source_root))
        .transpose()?;
    let generation = config.generation.clone();
    let routing = generation.routing.clone();
    let prompt = generation.prompt.clone();
    let image = generation.image.clone();
    let provider: Box<dyn PageGenerator> = match generation.provider_mode {
        ProviderMode::Mock => Box::new(MockProvider::with_routing(routing.clone())),
        ProviderMode::Real => Box::new(RealPageGenerator::from_env(generation)?),
    };
    let image_provider = image_provider_for(&config.generation);
    let existing_pages = match &site_root {
        Some(site_root) if site_root.exists() => load_existing_generated_pages(site_root)?,
        _ => BTreeMap::new(),
    };

    println!("source_sha={source_sha}");
    println!("directories={}", snapshot.directories.len());
    let outcome = run_generation(RunGenerationInput {
        snapshot: &snapshot,
        source_sha: &source_sha,
        site_root: site_root.as_deref(),
        routing: &routing,
        prompt: &prompt,
        image: &image,
        provider: provider.as_ref(),
        image_provider: image_provider.as_ref(),
        existing_pages: &existing_pages,
    })?;

    for line in spend_report_lines(&outcome.spend_report) {
        println!("{line}");
    }

    if !outcome.failures.is_empty() {
        bail!(
            "{} pages failed during generation; see run summary",
            outcome.failures.len()
        );
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunFailure {
    directory: PathBuf,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunOutcome {
    spend_report: SpendReport,
    failures: Vec<RunFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RunPageManifest {
    directory: String,
    kind: &'static str,
    provider: String,
    model: String,
    params: RunPageParams,
    prompt_version: String,
    catalog_version: String,
    context_blocks: Vec<RunContextBlock>,
    input_tokens: u64,
    output_tokens: u64,
    spend_micros: u64,
    image: RunImageManifest,
    retries: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RunPageParams {
    max_tokens: u32,
    temperature: Option<String>,
    output_contract: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RunContextBlock {
    name: String,
    byte_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
struct RunImageManifest {
    requested: usize,
    rendered: usize,
    failed: usize,
    skipped: usize,
    spend_micros: u64,
    files: Vec<String>,
}

struct RunGenerationInput<'a> {
    snapshot: &'a glance_core::DirectorySnapshot,
    source_sha: &'a str,
    site_root: Option<&'a Path>,
    routing: &'a glance_gen::DepthRouting,
    prompt: &'a glance_gen::PromptConfig,
    image: &'a glance_gen::ImageConfig,
    provider: &'a dyn PageGenerator,
    image_provider: &'a dyn ImageProvider,
    existing_pages: &'a BTreeMap<PathBuf, String>,
}

fn run_generation(input: RunGenerationInput<'_>) -> Result<RunOutcome> {
    let RunGenerationInput {
        snapshot,
        source_sha,
        site_root,
        routing,
        prompt,
        image,
        provider,
        image_provider,
        existing_pages,
    } = input;
    let mut spend_report = SpendReport::default();
    let mut generated_pages = BTreeMap::new();
    let mut degraded_pages = BTreeSet::new();
    let mut failures = Vec::new();
    let mut page_manifests = Vec::new();
    let mut image_budget = ImageBudget::new(image.budget_per_run);

    for directory in leaf_to_root_dirs(snapshot.directories.keys().cloned()) {
        let kind = if directory == Path::new(".") {
            PageKind::Root
        } else {
            let record = snapshot.directory(&directory).context("directory record")?;
            if record.child_dirs.is_empty() {
                PageKind::Leaf
            } else {
                PageKind::Interior
            }
        };
        let route = routing.model_for(kind);
        let prompt_context = match assemble_prompt_context_with_degraded(
            snapshot,
            &directory,
            kind,
            prompt.max_file_bytes,
            &generated_pages,
            existing_pages,
            &degraded_pages,
        ) {
            Ok(prompt_context) => prompt_context,
            Err(error) => {
                record_run_failure(&mut degraded_pages, &mut failures, &directory, error);
                continue;
            }
        };
        let mut page = match provider.generate(
            GenerationRequest::new(
                snapshot.source_root.clone(),
                directory.clone(),
                source_sha.to_owned(),
                kind,
            )
            .with_prompt_context(prompt_context),
        ) {
            Ok(page) => page,
            Err(error) => {
                record_run_failure(&mut degraded_pages, &mut failures, &directory, error);
                continue;
            }
        };
        match normalize_generated_html_citations(&page.html, &snapshot.source_root, &directory) {
            Ok(html) => page.html = html,
            Err(error) => {
                record_run_failure(&mut degraded_pages, &mut failures, &directory, error);
                continue;
            }
        }
        if let Err(error) = validate_rendered_page_before_write(snapshot, &page.html) {
            record_run_failure(&mut degraded_pages, &mut failures, &directory, error);
            continue;
        }
        spend_report.record(PageSpend {
            directory: directory.clone(),
            provider: page.provider.clone(),
            model: page.model.clone(),
            input_tokens: page.input_tokens,
            output_tokens: page.output_tokens,
            spend_micros: page.spend_micros,
        });
        println!(
            "would_generate={} kind={:?} tier={:?} provider={} model={} max_tokens={} input_tokens={} output_tokens={} spend_micros={}",
            directory.display(),
            kind,
            page.tier,
            page.provider,
            page.model,
            route.max_tokens,
            page.input_tokens,
            page.output_tokens,
            page.spend_micros
        );
        for note in &page.metadata_notes {
            println!("metadata_note={} {}", directory.display(), note);
        }
        let mut image_manifest = RunImageManifest::default();
        if let Some(site_root) = site_root {
            let output_dir = page_output_dir(site_root, &directory);
            let image_report = render_image_requests(
                &page.html,
                &output_dir,
                image,
                image_provider,
                &mut image_budget,
            );
            image_manifest = run_image_manifest(&image_report, &output_dir);
            page.html = image_report.html;
            if image_report.requested > 0 {
                println!(
                    "image_report={} requested={} rendered={} failed={} skipped={} remaining_budget={}",
                    directory.display(),
                    image_report.requested,
                    image_report.rendered,
                    image_report.failed,
                    image_report.skipped,
                    image_budget.remaining()
                );
                for message in image_report.messages {
                    println!("image_note={} {}", directory.display(), message);
                }
            }
            if let Err(error) = write_generated_page(site_root, &directory, source_sha, kind, &page)
            {
                record_run_failure(&mut degraded_pages, &mut failures, &directory, error);
                continue;
            }
            println!(
                "wrote_page={} {}",
                directory.display(),
                page_output_dir(site_root, &directory).display()
            );
        }
        page_manifests.push(run_page_manifest(
            &directory,
            kind,
            route.max_tokens,
            &page,
            image_manifest,
        ));
        generated_pages.insert(directory.clone(), page.html.clone());
    }

    if let Some(site_root) = site_root {
        write_run_summary(
            site_root,
            source_sha,
            generated_pages.len(),
            &failures,
            &page_manifests,
        )?;
    }

    for failure in &failures {
        println!(
            "run_failed_page={} error={}",
            failure.directory.display(),
            failure.message
        );
    }

    Ok(RunOutcome {
        spend_report,
        failures,
    })
}

fn image_provider_for(generation: &GenerationConfig) -> Box<dyn ImageProvider> {
    if generation.provider_mode == ProviderMode::Mock {
        return Box::new(MockImageProvider);
    }
    match generation.image.provider {
        ImageProviderKind::Mock => Box::new(MockImageProvider),
        ImageProviderKind::Gemini => match GeminiImageProvider::from_env(&generation.image) {
            Ok(provider) => Box::new(provider),
            Err(error) => Box::new(DisabledImageProvider {
                message: error.to_string(),
            }),
        },
        ImageProviderKind::GptImage2 => Box::new(DisabledImageProvider {
            message: "gpt-image-2 image provider is not implemented yet".to_owned(),
        }),
    }
}

struct DisabledImageProvider {
    message: String,
}

impl ImageProvider for DisabledImageProvider {
    fn render(
        &self,
        _request: &ImageRequest,
    ) -> std::result::Result<ImageOutput, glance_gen::GenerationError> {
        Err(glance_gen::GenerationError::Provider {
            provider: "image",
            retryable: false,
            message: self.message.clone(),
        })
    }
}

fn record_run_failure<E: std::fmt::Display>(
    degraded_pages: &mut BTreeSet<PathBuf>,
    failures: &mut Vec<RunFailure>,
    directory: &Path,
    error: E,
) {
    degraded_pages.insert(directory.to_path_buf());
    failures.push(RunFailure {
        directory: directory.to_path_buf(),
        message: error.to_string(),
    });
}

fn write_run_summary(
    site_root: &Path,
    source_sha: &str,
    pages_generated: usize,
    failures: &[RunFailure],
    pages: &[RunPageManifest],
) -> Result<()> {
    std::fs::create_dir_all(site_root)
        .with_context(|| format!("create site root {}", site_root.display()))?;
    let summary = json!({
        "source_sha": source_sha,
        "pages_generated": pages_generated,
        "pages": pages,
        "failed_pages": failures.iter().map(|failure| {
            json!({
                "directory": path_label(&failure.directory),
                "message": failure.message,
            })
        }).collect::<Vec<_>>(),
    });
    let summary = serde_json::to_string_pretty(&summary).context("serialize run summary")? + "\n";
    std::fs::write(site_root.join("run-summary.json"), summary)
        .with_context(|| format!("write {}", site_root.join("run-summary.json").display()))?;
    Ok(())
}

fn run_page_manifest(
    directory: &Path,
    kind: PageKind,
    max_tokens: u32,
    page: &GeneratedPage,
    image: RunImageManifest,
) -> RunPageManifest {
    RunPageManifest {
        directory: path_label(directory),
        kind: page_kind_label(kind),
        provider: page.provider.clone(),
        model: page.model.clone(),
        params: RunPageParams {
            max_tokens,
            temperature: None,
            output_contract: "json_object/glance-catalog-001",
        },
        prompt_version: page.prompt_version.clone(),
        catalog_version: page.catalog_version.clone(),
        context_blocks: context_block_manifest(&page.context_blocks),
        input_tokens: page.input_tokens,
        output_tokens: page.output_tokens,
        spend_micros: page.spend_micros,
        image,
        retries: page.retries,
    }
}

fn context_block_manifest(blocks: &[ContextBlockMetadata]) -> Vec<RunContextBlock> {
    blocks
        .iter()
        .map(|block| RunContextBlock {
            name: block.name.clone(),
            byte_size: block.byte_size,
        })
        .collect()
}

fn run_image_manifest(report: &ImageRenderReport, output_dir: &Path) -> RunImageManifest {
    RunImageManifest {
        requested: report.requested,
        rendered: report.rendered,
        failed: report.failed,
        skipped: report.skipped,
        spend_micros: report.spend_micros,
        files: report
            .files
            .iter()
            .map(|file| {
                file.strip_prefix(output_dir)
                    .unwrap_or(file.as_path())
                    .display()
                    .to_string()
            })
            .collect(),
    }
}

fn validate_rendered_page_before_write(
    snapshot: &glance_core::DirectorySnapshot,
    html: &str,
) -> Result<()> {
    let mut failures = Vec::new();

    for failure in validate_navigation(html, snapshot) {
        failures.push(format!(
            "navigation {}: {}",
            failure.directory.display(),
            failure.message
        ));
    }
    for failure in validate_page_contract(html) {
        failures.push(format!("page contract: {}", failure.message));
    }
    match Citation::from_html(html) {
        Ok(citations) => {
            for citation in citations {
                if let Err(error) = validate_live_citation_range(snapshot, &citation) {
                    failures.push(error);
                }
            }
        }
        Err(error) => failures.push(error.to_string()),
    }

    if failures.is_empty() {
        Ok(())
    } else {
        bail!(
            "rendered page failed pre-write validation: {}",
            failures.join("; ")
        )
    }
}

fn validate_live_citation_range(
    snapshot: &glance_core::DirectorySnapshot,
    citation: &Citation,
) -> std::result::Result<(), String> {
    let path = snapshot.source_root.join(&citation.path);
    let content = std::fs::read_to_string(&path)
        .map_err(|error| format!("{}: {error}", citation.path.display()))?;
    let line_count = content.lines().count();
    if citation.end_line > line_count {
        return Err(format!(
            "{} has {line_count} lines, citation asks for line {}",
            citation.path.display(),
            citation.end_line
        ));
    }
    Ok(())
}

fn resolve_site_root_outside_source(site_root: &Path, source_root: &Path) -> Result<PathBuf> {
    let source_root = source_root
        .canonicalize()
        .with_context(|| format!("canonicalize source root {}", source_root.display()))?;
    let site_root = canonicalize_writable_path(site_root)?;
    if site_root == source_root || site_root.starts_with(&source_root) {
        bail!(
            "site_root {} must be outside source_root {}; generated HTML is never written into the source repository",
            site_root.display(),
            source_root.display()
        );
    }
    Ok(site_root)
}

fn canonicalize_writable_path(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return path
            .canonicalize()
            .with_context(|| format!("canonicalize {}", path.display()));
    }

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("current directory")?
            .join(path)
    };
    let mut existing = absolute.as_path();
    let mut suffix = PathBuf::new();
    while !existing.exists() {
        let name = existing
            .file_name()
            .with_context(|| format!("find existing parent for {}", path.display()))?;
        let mut next_suffix = PathBuf::from(name);
        next_suffix.push(&suffix);
        suffix = next_suffix;
        existing = existing
            .parent()
            .with_context(|| format!("find existing parent for {}", path.display()))?;
    }

    let mut canonical = existing
        .canonicalize()
        .with_context(|| format!("canonicalize {}", existing.display()))?;
    canonical.push(suffix);
    Ok(canonical)
}

fn write_generated_page(
    site_root: &Path,
    directory: &Path,
    source_sha: &str,
    kind: PageKind,
    page: &GeneratedPage,
) -> Result<()> {
    let output_dir = page_output_dir(site_root, directory);
    std::fs::create_dir_all(&output_dir)
        .with_context(|| format!("create output dir {}", output_dir.display()))?;
    std::fs::write(output_dir.join("index.html"), &page.html)
        .with_context(|| format!("write {}", output_dir.join("index.html").display()))?;

    let metadata = json!({
        "source_sha": source_sha,
        "directory": path_label(directory),
        "kind": page_kind_label(kind),
        "prompt_version": &page.prompt_version,
        "catalog_version": &page.catalog_version,
        "provider": &page.provider,
        "model": &page.model,
        "tier": format!("{:?}", page.tier),
        "input_tokens": page.input_tokens,
        "output_tokens": page.output_tokens,
        "spend_micros": page.spend_micros,
        "retries": page.retries,
        "context_blocks": context_block_manifest(&page.context_blocks),
        "metadata_notes": &page.metadata_notes,
        "degraded_children": page.degraded_children.iter().map(|path| path_label(path)).collect::<Vec<_>>(),
    });
    let metadata = serde_json::to_string_pretty(&metadata).context("serialize metadata")? + "\n";
    std::fs::write(output_dir.join("metadata.json"), metadata)
        .with_context(|| format!("write {}", output_dir.join("metadata.json").display()))?;
    Ok(())
}

fn load_existing_generated_pages(site_root: &Path) -> Result<BTreeMap<PathBuf, String>> {
    let site_root = site_root
        .canonicalize()
        .with_context(|| format!("canonicalize site root {}", site_root.display()))?;
    let mut pages = BTreeMap::new();
    for html_file in find_html_files(&site_root)? {
        if html_file.file_name().and_then(|name| name.to_str()) != Some("index.html") {
            continue;
        }
        let parent = html_file.parent().context("index parent")?;
        let relative_parent = parent
            .strip_prefix(&site_root)
            .with_context(|| format!("make {} relative", parent.display()))?;
        let directory = if relative_parent.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            relative_parent.to_path_buf()
        };
        let html = std::fs::read_to_string(&html_file)
            .with_context(|| format!("read existing page {}", html_file.display()))?;
        pages.insert(directory, html);
    }
    Ok(pages)
}

fn page_output_dir(site_root: &Path, directory: &Path) -> PathBuf {
    if directory == Path::new(".") {
        site_root.to_path_buf()
    } else {
        site_root.join(directory)
    }
}

fn page_kind_label(kind: PageKind) -> &'static str {
    match kind {
        PageKind::Leaf => "leaf",
        PageKind::Interior => "interior",
        PageKind::Root | PageKind::CrossCutting => "root",
    }
}

fn path_label(path: &Path) -> String {
    if path == Path::new(".") {
        ".".to_owned()
    } else {
        path.display().to_string()
    }
}

fn plan_command(
    config: &GlanceConfig,
    root: Option<PathBuf>,
    changed_paths: Vec<PathBuf>,
) -> Result<()> {
    let root = root
        .or_else(|| config.source_root.clone())
        .unwrap_or_else(|| PathBuf::from("."));
    let changed_paths = if changed_paths.is_empty() {
        config.changed_paths.clone().unwrap_or_default()
    } else {
        changed_paths
    };

    if changed_paths.is_empty() {
        let source_sha =
            configured_or_git_sha(config, &root).unwrap_or_else(|_| "WORKTREE".to_owned());
        let snapshot = snapshot_tree(&root, source_sha)?;
        for directory in leaf_to_root_dirs(snapshot.directories.keys().cloned()) {
            println!("{}", directory.display());
        }
        return Ok(());
    }

    let plan = RegenerationPlan::from_changed_paths(&root, changed_paths)?;
    for directory in plan.directories {
        println!("{}", directory.display());
    }
    Ok(())
}

fn check_command(
    config: &GlanceConfig,
    source_root: Option<PathBuf>,
    source_sha: Option<String>,
    html: Vec<PathBuf>,
) -> Result<()> {
    let source_root = source_root
        .or_else(|| config.source_root.clone())
        .unwrap_or_else(|| PathBuf::from("."));
    let source_sha = match source_sha.or_else(|| config.source_sha.clone()) {
        Some(source_sha) => source_sha,
        None => configured_or_git_sha(config, &source_root)?,
    };
    let html_files = if html.is_empty() {
        let site_root = config
            .site_root
            .clone()
            .context("no HTML files supplied and no site_root in config")?;
        find_html_files(&site_root)?
    } else {
        html
    };

    let checker = CitationChecker::new(&source_root, source_sha);
    let mut total_citations = 0;
    let mut total_failures = 0;
    let mut total_navigation_failures = 0;
    let mut total_page_contract_failures = 0;

    for html_file in html_files {
        let report = checker.check_html_file(&html_file)?;
        total_citations += report.citations_checked;
        total_failures += report.failures.len();
        total_navigation_failures += report.navigation_failures.len();
        total_page_contract_failures += report.page_contract_failures.len();
        if report.is_ok() {
            println!(
                "ok {} citations={}",
                html_file.display(),
                report.citations_checked
            );
        } else {
            let page_failures = report.failures.len()
                + report.navigation_failures.len()
                + report.page_contract_failures.len();
            println!(
                "fail {} citations={} failures={page_failures}",
                html_file.display(),
                report.citations_checked
            );
            for failure in report.failures {
                println!(
                    "  {}:{}-{} {}",
                    failure.citation.path.display(),
                    failure.citation.start_line,
                    failure.citation.end_line,
                    failure.message
                );
            }
            for failure in report.navigation_failures {
                println!(
                    "  navigation {} {}",
                    failure.directory.display(),
                    failure.message
                );
            }
            for failure in report.page_contract_failures {
                println!("  page_contract {}", failure.message);
            }
        }
    }

    if total_failures > 0 || total_navigation_failures > 0 || total_page_contract_failures > 0 {
        bail!(
            "{} validation failures across {total_citations} checked citations (citation_failures={total_failures}, navigation_failures={total_navigation_failures}, page_contract_failures={total_page_contract_failures})",
            total_failures + total_navigation_failures + total_page_contract_failures
        );
    }

    println!("checked {total_citations} citations and navigation");
    Ok(())
}

fn serve_local_command(
    config: &GlanceConfig,
    site_root: Option<PathBuf>,
    port: u16,
    once: bool,
) -> Result<()> {
    let site_root = site_root
        .or_else(|| config.site_root.clone())
        .unwrap_or_else(|| PathBuf::from("site"));
    let site_root = site_root
        .canonicalize()
        .with_context(|| format!("canonicalize site root {}", site_root.display()))?;
    let listener = TcpListener::bind(("127.0.0.1", port)).context("bind local server")?;
    let address = listener.local_addr().context("local address")?;
    println!("serving {} at http://{address}", site_root.display());

    for stream in listener.incoming() {
        handle_connection(stream.context("incoming connection")?, &site_root)?;
        if once {
            break;
        }
    }
    Ok(())
}

fn publish_command(command: PublishCommand) -> Result<()> {
    let source = SourceRepo {
        owner: command.source_owner,
        name: command.source_name,
        sha: command.source_sha,
    };
    let worktree_dir = command.sister_worktree.unwrap_or_else(|| {
        PathBuf::from("target")
            .join("glance-publish")
            .join(source.sister_name())
    });
    let mode = match command.mode {
        PublishModeArg::Master => glance_publish::PublishMode::Master,
        PublishModeArg::Branch => {
            let branch = command
                .branch
                .unwrap_or_else(|| format!("glance/{}", short_sha(&source.sha)));
            let pr_title = command
                .source_pr_title
                .context("--source-pr-title is required for --mode branch")?;
            glance_publish::PublishMode::Branch { branch, pr_title }
        }
    };

    let outcome = glance_publish::publish(
        PublishRequest {
            site_dir: command.site_dir,
            source,
            worktree_dir,
            sister_remote: command.sister_remote,
            mode,
            run_id: command.run_id,
        },
        &GhSisterHost,
    )?;

    println!("changed={}", outcome.changed);
    println!("sister_ref={}", outcome.pushed_ref);
    println!("worktree={}", outcome.worktree_dir.display());
    if let Some(commit_sha) = outcome.commit_sha {
        println!("commit_sha={commit_sha}");
    }
    if let Some(pr_url) = outcome.pr_url {
        println!("pr_url={pr_url}");
    }

    Ok(())
}

fn short_sha(sha: &str) -> &str {
    sha.get(..12).unwrap_or(sha)
}

fn handle_connection(mut stream: TcpStream, site_root: &Path) -> Result<()> {
    let mut buffer = [0; 1024];
    let read = stream.read(&mut buffer).context("read request")?;
    let request = String::from_utf8_lossy(&buffer[..read]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    let relative = match request_path_to_relative(path) {
        Some(relative) => relative,
        None => {
            write_response(&mut stream, "403 Forbidden", b"forbidden")?;
            return Ok(());
        }
    };
    let mut file_path = if relative.as_os_str().is_empty() {
        site_root.join("index.html")
    } else {
        site_root.join(relative)
    };
    if file_path.is_dir() {
        file_path = file_path.join("index.html");
    }

    match file_path.canonicalize() {
        Ok(canonical) if canonical.starts_with(site_root) => {
            let bytes = std::fs::read(canonical).context("read served file")?;
            write_response(&mut stream, "200 OK", &bytes)?;
        }
        Ok(_) => write_response(&mut stream, "403 Forbidden", b"forbidden")?,
        Err(_) => write_response(&mut stream, "404 Not Found", b"not found")?,
    }
    Ok(())
}

fn request_path_to_relative(path: &str) -> Option<PathBuf> {
    let path = path
        .split('?')
        .next()
        .unwrap_or(path)
        .trim_start_matches('/');
    let mut relative = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(relative)
}

fn write_response(stream: &mut TcpStream, status: &str, body: &[u8]) -> Result<()> {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .context("write header")?;
    stream.write_all(body).context("write body")?;
    Ok(())
}

fn load_config(path: &Path) -> Result<GlanceConfig> {
    if !path.exists() {
        return Ok(GlanceConfig::default());
    }
    let content =
        std::fs::read_to_string(path).with_context(|| format!("read config {}", path.display()))?;
    toml::from_str(&content).with_context(|| format!("parse config {}", path.display()))
}

fn configured_or_git_sha(config: &GlanceConfig, root: &Path) -> Result<String> {
    if let Some(source_sha) = &config.source_sha {
        Ok(source_sha.clone())
    } else {
        Ok(SourcePin::resolve_git_head(root)?.sha)
    }
}

fn find_html_files(root: &Path) -> Result<Vec<PathBuf>> {
    let root = root
        .canonicalize()
        .with_context(|| format!("canonicalize HTML root {}", root.display()))?;
    let mut files = Vec::new();
    collect_html_files(&root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_html_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(path).with_context(|| format!("read {}", path.display()))? {
        let entry = entry?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)
            .with_context(|| format!("read metadata {}", path.display()))?;
        if metadata.file_type().is_dir() {
            collect_html_files(&path, files)?;
        } else if metadata.file_type().is_file()
            && path.extension().and_then(|extension| extension.to_str()) == Some("html")
        {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_paths_reject_parent_traversal() {
        assert!(request_path_to_relative("/../secret").is_none());
        assert_eq!(
            request_path_to_relative("/nested/index.html?x=1").expect("relative"),
            PathBuf::from("nested/index.html")
        );
    }

    #[test]
    fn run_site_root_must_be_outside_source_root() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("source");
        std::fs::create_dir(&source).expect("source");

        assert!(resolve_site_root_outside_source(&source, &source).is_err());
        assert!(resolve_site_root_outside_source(&source.join("site"), &source).is_err());
        assert!(
            resolve_site_root_outside_source(&temp.path().join("site"), &source)
                .expect("sibling site")
                .ends_with("site")
        );
    }

    #[test]
    fn run_generation_continues_after_one_page_failure_and_records_degraded_parent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("source");
        std::fs::create_dir(&source).expect("source");
        std::fs::create_dir(source.join("ok")).expect("ok");
        std::fs::create_dir(source.join("bad")).expect("bad");
        std::fs::write(source.join("README.md"), "# fixture\n").expect("readme");
        std::fs::write(source.join("ok/lib.rs"), "pub fn ok() {}\n").expect("ok source");
        std::fs::write(source.join("bad/lib.rs"), "pub fn bad() {}\n").expect("bad source");
        let snapshot = snapshot_tree(&source, "fixture-sha").expect("snapshot");
        let site = temp.path().join("site");
        let provider = FailsOneDirectoryProvider {
            failing_directory: PathBuf::from("bad"),
        };
        let existing_pages = BTreeMap::new();

        let config = GenerationConfig::default();
        let outcome = run_generation(RunGenerationInput {
            snapshot: &snapshot,
            source_sha: "fixture-sha",
            site_root: Some(&site),
            routing: &config.routing,
            prompt: &config.prompt,
            image: &config.image,
            provider: &provider,
            image_provider: &MockImageProvider,
            existing_pages: &existing_pages,
        })
        .expect("run should complete with recorded failure");

        assert_eq!(outcome.failures.len(), 1);
        assert_eq!(outcome.failures[0].directory, PathBuf::from("bad"));
        assert!(site.join("ok/index.html").is_file());
        assert!(!site.join("bad/index.html").exists());
        assert!(site.join("index.html").is_file());
        let root_metadata = std::fs::read_to_string(site.join("metadata.json"))
            .expect("root metadata")
            .parse::<serde_json::Value>()
            .expect("metadata json");
        assert_eq!(root_metadata["degraded_children"], json!(["bad"]));
        let summary = std::fs::read_to_string(site.join("run-summary.json"))
            .expect("summary")
            .parse::<serde_json::Value>()
            .expect("summary json");
        assert_eq!(summary["failed_pages"][0]["directory"], "bad");
        let pages = summary["pages"].as_array().expect("pages manifest");
        assert!(pages.iter().any(|page| {
            page["directory"] == "ok"
                && page["catalog_version"] == glance_gen::CATALOG_VERSION
                && page["params"]["max_tokens"].as_u64() == Some(6_000)
                && page["context_blocks"]
                    .as_array()
                    .is_some_and(|blocks| !blocks.is_empty())
        }));
    }

    #[test]
    fn run_generation_rejects_out_of_range_citations_before_writing_page() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("source");
        std::fs::create_dir(&source).expect("source");
        std::fs::write(source.join("README.md"), "# fixture\n").expect("readme");
        let snapshot = snapshot_tree(&source, "fixture-sha").expect("snapshot");
        let site = temp.path().join("site");
        let config = GenerationConfig::default();
        let provider = OutOfRangeCitationProvider;
        let existing_pages = BTreeMap::new();

        let outcome = run_generation(RunGenerationInput {
            snapshot: &snapshot,
            source_sha: "fixture-sha",
            site_root: Some(&site),
            routing: &config.routing,
            prompt: &config.prompt,
            image: &config.image,
            provider: &provider,
            image_provider: &MockImageProvider,
            existing_pages: &existing_pages,
        })
        .expect("run records validation failure");

        assert_eq!(outcome.failures.len(), 1);
        assert!(
            outcome.failures[0]
                .message
                .contains("README.md has 1 lines")
        );
        assert!(!site.join("index.html").exists());
        let summary = std::fs::read_to_string(site.join("run-summary.json"))
            .expect("summary")
            .parse::<serde_json::Value>()
            .expect("summary json");
        assert_eq!(summary["pages_generated"], 0);
        assert_eq!(summary["failed_pages"][0]["directory"], ".");
    }

    #[test]
    fn mock_run_writes_navigation_links_that_resolve_to_generated_pages() {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../glance-core/tests/fixtures/mini-source");
        let snapshot = snapshot_tree(&source, "fixture-sha").expect("snapshot");
        let site = tempfile::tempdir().expect("site");
        let config = GenerationConfig::default();
        let existing_pages = BTreeMap::new();

        let provider = MockProvider::with_routing(config.routing.clone());
        let outcome = run_generation(RunGenerationInput {
            snapshot: &snapshot,
            source_sha: "fixture-sha",
            site_root: Some(site.path()),
            routing: &config.routing,
            prompt: &config.prompt,
            image: &config.image,
            provider: &provider,
            image_provider: &MockImageProvider,
            existing_pages: &existing_pages,
        })
        .expect("mock run");

        assert!(outcome.failures.is_empty(), "{outcome:#?}");
        let root_html = std::fs::read_to_string(site.path().join("index.html")).expect("root");
        assert!(root_html.contains(r#"href="docs/index.html""#));
        assert!(root_html.contains(r#"href="src/index.html""#));
        assert!(site.path().join("docs/index.html").is_file());
        assert!(site.path().join("src/index.html").is_file());

        let src_html = std::fs::read_to_string(site.path().join("src/index.html")).expect("src");
        assert!(src_html.contains(r#"href="../index.html""#));
        assert!(src_html.contains(r#"href="parser/index.html""#));
        assert!(site.path().join("src/parser/index.html").is_file());

        let parser_html =
            std::fs::read_to_string(site.path().join("src/parser/index.html")).expect("parser");
        assert!(parser_html.contains(r#"href="../index.html""#));

        assert!(root_html.contains("glance-flow-diagram"));
        assert!(root_html.contains("data-glance-catalog-version=\"glance-catalog-001\""));
        assert!(root_html.contains("data-theme-choice=\"system\""));
        // Mock mode has no real image to show: MockImageProvider's output is
        // deliberately too small to pass `image_meets_minimum_dimensions`, so
        // the root page keeps the honest fallback figure, never a fake <img>.
        assert!(root_html.contains("glance-image-fallback"));
        assert!(!root_html.contains("<img "));
        assert!(!site.path().join("glance-image-001.png").exists());

        let summary = std::fs::read_to_string(site.path().join("run-summary.json"))
            .expect("summary")
            .parse::<serde_json::Value>()
            .expect("summary json");
        let root_page = summary["pages"]
            .as_array()
            .expect("pages")
            .iter()
            .find(|page| page["directory"] == ".")
            .expect("root page");
        assert_eq!(root_page["image"]["spend_micros"], 0);
    }

    #[cfg(unix)]
    #[test]
    fn html_scan_does_not_follow_symlinked_directories() {
        use std::os::unix::fs::symlink;

        let site = tempfile::tempdir().expect("site");
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(site.path().join("index.html"), "<html></html>").expect("site html");
        std::fs::write(outside.path().join("outside.html"), "<html></html>").expect("outside html");
        symlink(outside.path(), site.path().join("linked")).expect("symlink");

        let files = find_html_files(site.path()).expect("files");

        assert_eq!(
            files,
            vec![site.path().join("index.html").canonicalize().unwrap()]
        );
    }

    struct FailsOneDirectoryProvider {
        failing_directory: PathBuf,
    }

    impl PageGenerator for FailsOneDirectoryProvider {
        fn generate(
            &self,
            request: GenerationRequest,
        ) -> std::result::Result<GeneratedPage, glance_gen::GenerationError> {
            if request.directory == self.failing_directory {
                return Err(glance_gen::GenerationError::Provider {
                    provider: "test",
                    retryable: false,
                    message: "planned failure".to_owned(),
                });
            }
            MockProvider::default().generate(request)
        }
    }

    struct OutOfRangeCitationProvider;

    impl PageGenerator for OutOfRangeCitationProvider {
        fn generate(
            &self,
            request: GenerationRequest,
        ) -> std::result::Result<GeneratedPage, glance_gen::GenerationError> {
            let mut page = MockProvider::default().generate(request)?;
            page.html = page.html.replace("README.md:1-1", "README.md:1-99");
            Ok(page)
        }
    }
}

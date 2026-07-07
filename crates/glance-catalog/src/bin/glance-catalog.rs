//! glance-catalog's producer seam (glance-929, glance-922 criterion 2): a
//! CLI that reads a `glance_catalog::document::CatalogDocument` spec as JSON
//! (stdin or `--input`) and writes the rendered, self-contained HTML (stdout
//! or `--output`) -- so a non-Rust producer (a Python script, a shell
//! pipeline) can call this crate's renderer via subprocess with no Rust
//! dependency of its own. See `docs/producer-cli.md` for the full contract
//! (schema location/version, exit codes, error shapes) an external producer
//! integrates against; this file is the thin spine that contract describes,
//! not the place to read it from.
//!
//! Citation verification is opt-in and reuses `glance_check::CitationChecker`
//! (not a parallel implementation): supply `--source-root`/`--source-sha`
//! together to fail closed on any `Cite` node whose `path:lines` ref_id
//! doesn't resolve against that pinned commit. Omit both when a document's
//! `Cite` nodes use a different ref_id scheme (e.g. fleet-retro's opaque
//! evidence-pack ids) -- this crate's `Cite` type is deliberately
//! scheme-agnostic (see `glance_catalog::inline`), so verification against
//! `glance_check`'s `path:lines` grammar is exactly this opt-in, never
//! forced on every consumer.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use chrono::Utc;
use glance_catalog::document::CatalogDocument;
use glance_catalog::render::RenderContext;

struct Args {
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    source_root: Option<PathBuf>,
    source_sha: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut input = None;
    let mut output = None;
    let mut source_root = None;
    let mut source_sha = None;
    let mut raw = std::env::args().skip(1);
    while let Some(flag) = raw.next() {
        let mut value = || raw.next().ok_or_else(|| format!("{flag} requires a value"));
        match flag.as_str() {
            "--input" => input = Some(PathBuf::from(value()?)),
            "--output" => output = Some(PathBuf::from(value()?)),
            "--source-root" => source_root = Some(PathBuf::from(value()?)),
            "--source-sha" => source_sha = Some(value()?),
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }
    if source_root.is_some() != source_sha.is_some() {
        return Err("--source-root and --source-sha must be given together".to_string());
    }
    Ok(Args {
        input,
        output,
        source_root,
        source_sha,
    })
}

fn read_input(path: Option<&PathBuf>) -> Result<String, String> {
    match path {
        Some(path) => std::fs::read_to_string(path)
            .map_err(|error| format!("reading {}: {error}", path.display())),
        None => {
            let mut buffer = String::new();
            std::io::stdin()
                .read_to_string(&mut buffer)
                .map_err(|error| format!("reading stdin: {error}"))?;
            Ok(buffer)
        }
    }
}

fn write_output(path: Option<&PathBuf>, html: &str) -> Result<(), String> {
    match path {
        Some(path) => std::fs::write(path, html)
            .map_err(|error| format!("writing {}: {error}", path.display())),
        None => std::io::stdout()
            .write_all(html.as_bytes())
            .map_err(|error| format!("writing stdout: {error}")),
    }
}

fn run() -> Result<(), (ExitCode, String)> {
    let args = parse_args().map_err(|message| (ExitCode::from(1), message))?;

    let json = read_input(args.input.as_ref()).map_err(|message| (ExitCode::from(1), message))?;

    let document = CatalogDocument::from_json(&json)
        .map_err(|error| (ExitCode::from(2), format!("invalid spec: {error}")))?;
    document
        .validate()
        .map_err(|error| (ExitCode::from(2), format!("invalid spec: {error}")))?;

    let ctx = RenderContext {
        now: Utc::now(),
        cite_href: &|ref_id| format!("#cite-{ref_id}"),
        cite_class: None,
        cite_label: None,
    };
    let html = document.render(&ctx);

    if let (Some(source_root), Some(source_sha)) = (&args.source_root, &args.source_sha) {
        let checker = glance_check::CitationChecker::new(source_root, source_sha.clone());
        let failures = checker
            .check_citations(&html)
            .map_err(|error| (ExitCode::from(3), format!("citation check failed: {error}")))?;
        if !failures.is_empty() {
            let mut message = format!("citation check failed: {} citation(s)\n", failures.len());
            for failure in &failures {
                message.push_str(&format!(
                    "  - {}:{}-{}: {}\n",
                    failure.citation.path.display(),
                    failure.citation.start_line,
                    failure.citation.end_line,
                    failure.message
                ));
            }
            return Err((ExitCode::from(3), message));
        }
    }

    write_output(args.output.as_ref(), &html).map_err(|message| (ExitCode::from(1), message))?;
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, message)) => {
            eprintln!("error: {message}");
            code
        }
    }
}

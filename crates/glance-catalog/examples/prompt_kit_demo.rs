//! Renders the aesthetic prompt kit's worked example (aesthetic-929): reads
//! a JSON file shaped like `{"catalog_version": ..., "components": [...]}`
//! -- the exact envelope `catalog.schema.json` describes -- deserializes it
//! into `Vec<Component>`, calls `.validate()` on every element (the two
//! Rust-only rules `catalog.schema.json` cannot express: `Table`'s
//! `empty_note` arity and `Disclosure`'s no-nested-hero-or-disclosure
//! rule), then `render_component()`, the same two calls this crate's real
//! consumers (glance-next, fleet-retro) make. Prints a self-contained HTML
//! page to stdout; nothing here is committed to source control -- see
//! aesthetic's `docs/prompt-kit/README.md` for why.
//!
//! Run: `cargo run --example prompt_kit_demo -p glance-catalog -- path/to/emitted.json > /tmp/rendered.html`

use std::env;
use std::fs;
use std::process::ExitCode;

use chrono::Utc;
use glance_catalog::{CATALOG_VERSION, Component, RenderContext, render_component};
use serde::Deserialize;

#[derive(Deserialize)]
struct Envelope {
    catalog_version: String,
    components: Vec<Component>,
}

fn cite_href(ref_id: &str) -> String {
    format!("#cite-{ref_id}")
}

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: prompt_kit_demo <path-to-emitted.json>");
        return ExitCode::FAILURE;
    };

    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) => {
            eprintln!("reading {path}: {err}");
            return ExitCode::FAILURE;
        }
    };

    let envelope: Envelope = match serde_json::from_str(&raw) {
        Ok(envelope) => envelope,
        Err(err) => {
            eprintln!("deserializing {path} as Vec<Component>: {err}");
            return ExitCode::FAILURE;
        }
    };

    if envelope.catalog_version != CATALOG_VERSION {
        eprintln!(
            "catalog_version mismatch: file says {:?}, this crate is {CATALOG_VERSION:?}",
            envelope.catalog_version
        );
        return ExitCode::FAILURE;
    }

    for component in &envelope.components {
        if let Err(err) = component.validate() {
            eprintln!("{} failed validate(): {err}", component.kind_name());
            return ExitCode::FAILURE;
        }
    }

    let ctx = RenderContext {
        now: Utc::now(),
        cite_href: &cite_href,
        cite_class: None,
        cite_label: None,
    };

    let mut body = String::new();
    for component in &envelope.components {
        body.push_str(&format!(
            r#"<section class="exemplar-entry"><div class="exemplar-meta"><code class="ae-strong">{}</code></div><div class="exemplar-frame">{}</div></section>"#,
            component.kind_name(),
            render_component(component, &ctx)
        ));
    }

    let page = format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>aesthetic-929 prompt kit -- worked example</title>
<link rel="stylesheet" href="aesthetic.css">
<style>
.exemplar-page{{max-width:var(--ae-measure-wide);margin:0 auto;padding:var(--ae-space-6) var(--ae-space-4);}}
.exemplar-entry{{margin:var(--ae-space-5) 0;}}
.exemplar-meta{{margin-bottom:var(--ae-space-2);font-size:13px;}}
.exemplar-frame{{border:1px solid var(--ae-line);border-radius:.5rem;padding:var(--ae-space-4);}}
</style>
</head><body><main class="exemplar-page">{body}</main></body></html>"#
    );
    println!("{page}");

    eprintln!(
        "prompt_kit_demo: {} component(s) validated and rendered clean",
        envelope.components.len()
    );
    ExitCode::SUCCESS
}

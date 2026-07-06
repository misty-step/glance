//! Generates the catalog's own documentation page: one rendered exemplar
//! per primitive (aesthetic-926 acceptance criterion 1: "Component catalog
//! declared (JSON specs + rendered exemplars for every primitive)").
//!
//! Prints a self-contained HTML page to stdout. `aesthetic.css` is linked,
//! not inlined -- the publish step (outside this crate, since "generated
//! HTML belongs in sister repos, never in a source repository" per this
//! repo's own AGENTS.md) inlines the real kit CSS for static hosting.
//!
//! Run: `cargo run --example exemplar -p glance-catalog > exemplar.html`

use chrono::{DateTime, Utc};
use glance_catalog::inline::InlineNode;
use glance_catalog::leaf::{
    Callout, CalloutKind, Code, Diff, Image, Markdown, Mermaid, Metric, Terminal,
};
use glance_catalog::structural::{
    Cell, CellValue, ColumnSpec, Disclosure, Hero, Narrative, NarrativeStatus, Row, Table,
    Timeline, TimelineEntry,
};
use glance_catalog::{Component, RenderContext, render_component};

fn text(s: &str) -> Vec<InlineNode> {
    vec![InlineNode::Text { text: s.into() }]
}

struct Entry {
    kind: &'static str,
    tier: &'static str,
    steal_from: &'static str,
    component: Component,
}

fn entries() -> Vec<Entry> {
    vec![
        Entry {
            kind: "markdown",
            tier: "leaf",
            steal_from: "Sideshow, verbatim",
            component: Component::Markdown(Markdown {
                content: "Prose, tables, and fenced code **as text** -- rendered through a real CommonMark parser, not escape-and-wrap.\n\n- a bullet\n- another".into(),
            }),
        },
        Entry {
            kind: "code",
            tier: "leaf",
            steal_from: "Sideshow + glance's citation-aware inline nodes",
            component: Component::Code(Code {
                language: Some("rust".into()),
                content: "pub fn tier(&self) -> Tier {\n    match self { .. }\n}".into(),
                cite_ref_id: None,
            }),
        },
        Entry {
            kind: "diff",
            tier: "leaf",
            steal_from: "Sideshow, verbatim -- largest measured token win after mermaid",
            component: Component::Diff(Diff {
                unified: "-fn old() -> bool { false }\n+fn new() -> bool { true }".into(),
            }),
        },
        Entry {
            kind: "terminal",
            tier: "leaf",
            steal_from: "Sideshow, verbatim (SGR-only, not a full TUI emulator)",
            component: Component::Terminal(Terminal {
                content: "$ cargo test -p glance-catalog\ntest result: ok. 40 passed".into(),
            }),
        },
        Entry {
            kind: "image",
            tier: "leaf",
            steal_from: "Sideshow, verbatim (SHA256-as-id dedup)",
            component: Component::Image(Image {
                asset_id: "sha256:9f1c…3a02".into(),
                alt: "labeled architecture diagram".into(),
                caption: Some("Figure 1: the two-tier catalog".into()),
            }),
        },
        Entry {
            kind: "mermaid",
            tier: "leaf",
            steal_from: "Sideshow, verbatim -- retires glance-gen's hand-rolled SVG renderer",
            component: Component::Mermaid(Mermaid {
                source: "graph TD; Leaf-->Structural; Structural-->Report;".into(),
            }),
        },
        Entry {
            kind: "metric",
            tier: "leaf",
            steal_from: "Unifies glance's StatChip + fleet-retro's StatCallout",
            component: Component::Metric(Metric {
                label: "Consumers".into(),
                value: "3".into(),
            }),
        },
        Entry {
            kind: "callout",
            tier: "leaf",
            steal_from: "glance-next, kept as-is -- no Sideshow equivalent",
            component: Component::Callout(Callout {
                kind: CalloutKind::Seam,
                title: "Where a consumer plugs in".into(),
                body: text("Table's demoted_note is the seam a consumer uses to fold zero-signal rows into one muted line instead of dead table rows."),
            }),
        },
        Entry {
            kind: "hero",
            tier: "structural",
            steal_from: "Convergently built 3x (glance, fleet-retro, Sideshow's implicit session header) -- merged into one struct",
            component: Component::Hero(Hero {
                title: "Fleet retro — daily".into(),
                summary: text("24h window, all sources swept"),
                stats: vec![
                    Metric { label: "Commits".into(), value: "27".into() },
                    Metric { label: "PRs".into(), value: "2".into() },
                ],
                image_intent: None,
            }),
        },
        Entry {
            kind: "narrative",
            tier: "structural",
            steal_from: "glance-next's InlineNode citation model -- the one piece of real IP here",
            component: Component::Narrative(Narrative {
                heading: "What mattered".into(),
                status: NarrativeStatus::Ok {
                    paragraphs: vec![vec![
                        InlineNode::Text { text: "The catalog consolidation shipped ".into() },
                        InlineNode::Cite { text: "[aesthetic-926]".into(), ref_id: "card:aesthetic-926".into() },
                        InlineNode::Text { text: ".".into() },
                    ]],
                },
            }),
        },
        Entry {
            kind: "table",
            tier: "structural",
            steal_from: "Generalizes glance's FileTable + fleet-retro's RepoActivityTable into one column schema",
            component: Component::Table(Table {
                heading: "Repo activity".into(),
                columns: vec![
                    ColumnSpec { key: "repo".into(), label: "repo".into(), numeric: false, emphasize: true },
                    ColumnSpec { key: "commits".into(), label: "commits".into(), numeric: true, emphasize: false },
                ],
                rows: vec![Row {
                    cells: vec![
                        Cell { column_key: "repo".into(), value: CellValue::Text { text: "glance".into() } },
                        Cell { column_key: "commits".into(), value: CellValue::Text { text: "5".into() } },
                    ],
                }],
                empty_note: None,
                demoted_note: Some("1 repo(s) swept with no activity: weave".into()),
            }),
        },
        Entry {
            kind: "timeline",
            tier: "structural",
            steal_from: "Merges fleet-retro's Timeline + Sideshow's half-finished trace",
            component: Component::Timeline(Timeline {
                heading: "Timeline".into(),
                entries: vec![TimelineEntry {
                    at: "2026-07-06T18:00:00Z".into(),
                    actor: "glance".into(),
                    kind: "pr-merged".into(),
                    summary: "glance-catalog crate merged".into(),
                    link: Some("https://github.com/misty-step/glance/pull/14".into()),
                    detail: text("Full trace: 40 unit tests, fmt/clippy clean, workspace gate green."),
                }],
                empty_note: None,
            }),
        },
        Entry {
            kind: "disclosure",
            tier: "structural",
            steal_from: "glance-next, kept as-is",
            component: Component::Disclosure(Disclosure {
                heading: "Design notes (click to expand)".into(),
                children: vec![Component::Markdown(Markdown {
                    content: "Disclosure children cannot contain a nested hero or disclosure -- enforced by validate().".into(),
                })],
            }),
        },
    ]
}

fn cite_href(ref_id: &str) -> String {
    format!("#cite-{ref_id}")
}

fn main() {
    let ctx = RenderContext {
        now: DateTime::parse_from_rfc3339("2026-07-06T21:00:00Z")
            .unwrap()
            .with_timezone(&Utc),
        cite_href: &cite_href,
    };

    let mut body = String::new();
    body.push_str(
        r#"<header class="ae-hero"><h1 class="ae-strong">Aesthetic Report Component Catalog</h1><p class="ae-dim">aesthetic-926 -- 8 leaf content primitives + 5 structural report-grammar primitives, one Rust crate (glance-catalog), one JSON schema, per-consumer layout profiles (STREAM/REPORT). Every rendered block below is the crate's own render_component() output, not a mockup.</p></header>"#,
    );

    let mut current_tier = "";
    for entry in entries() {
        if entry.tier != current_tier {
            current_tier = entry.tier;
            let label = if current_tier == "leaf" {
                "Tier 1 — leaf surfaces (content containers)"
            } else {
                "Tier 2 — structural components (report grammar)"
            };
            body.push_str(&format!(r#"<h2 class="ae-section-title">{label}</h2>"#));
        }
        body.push_str(&format!(
            r#"<section class="exemplar-entry"><div class="exemplar-meta"><code class="ae-strong">{}</code><span class="ae-dim"> -- {}</span></div><div class="exemplar-frame">{}</div></section>"#,
            entry.kind,
            entry.steal_from,
            render_component(&entry.component, &ctx)
        ));
    }

    let page = format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Aesthetic Report Component Catalog</title>
<link rel="stylesheet" href="aesthetic.css">
<style>
.exemplar-page{{max-width:var(--ae-measure-wide);margin:0 auto;padding:var(--ae-space-6) var(--ae-space-4);}}
.exemplar-entry{{margin:var(--ae-space-5) 0;}}
.exemplar-meta{{margin-bottom:var(--ae-space-2);font-size:13px;}}
.exemplar-frame{{border:1px solid var(--ae-line);border-radius:.5rem;padding:var(--ae-space-4);}}
.ae-section-title{{margin-top:var(--ae-space-6);font-size:16px;font-weight:var(--ae-w-medium);}}
.ae-cite{{font-family:var(--ae-font-mono);font-size:13px;color:var(--ae-accent);text-decoration:none;}}
</style>
</head><body><main class="exemplar-page">{body}</main></body></html>"#
    );
    println!("{page}");
}

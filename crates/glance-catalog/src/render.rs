//! One shared HTML renderer for every primitive in the catalog -- the
//! actual "MERGE don't arbitrate" deliverable. fleet-retro's structural
//! components (Hero, Narrative, Table, Timeline) render through these
//! functions instead of a second hand-rolled implementation; leaf
//! primitives render here too so the catalog's own exemplar page has a
//! real rendering for all 13 kinds, not a stub.
//!
//! Every function emits the Aesthetic kit's existing `.ae-*` vocabulary
//! (`.ae-stat-badges`, `.ae-table`/`.ae-plate`, `.ae-trail`, `.ae-wall`,
//! `.ae-tag`) -- the same classes fleet-retro's aesthetic-927 pass already
//! adopted -- rather than inventing a parallel set only this crate uses.

use chrono::{DateTime, Utc};

use crate::component::Component;
use crate::inline::{html_escape, render_inline_nodes};
use crate::leaf::{Callout, Code, Diff, Image, Markdown, Mermaid, Metric, Terminal};
use crate::structural::{
    Disclosure, Hero, Narrative, NarrativeStatus, Table, Timeline, render_cell,
};
use crate::time::relative_time;

pub struct RenderContext<'a> {
    /// The report's own generation time -- relative-time strings ("16h
    /// ago") are computed against this, not the wall clock, so a rendered
    /// page stays a pure function of its data.
    pub now: DateTime<Utc>,
    /// Resolves a `Cite` node's opaque `ref_id` to a concrete href. See
    /// `crate::inline` for why this is a callback, not a struct field.
    pub cite_href: &'a dyn Fn(&str) -> String,
}

pub fn render_component(component: &Component, ctx: &RenderContext<'_>) -> String {
    match component {
        Component::Markdown(inner) => render_markdown(inner),
        Component::Code(inner) => render_code(inner),
        Component::Diff(inner) => render_diff(inner),
        Component::Terminal(inner) => render_terminal(inner),
        Component::Image(inner) => render_image(inner),
        Component::Mermaid(inner) => render_mermaid(inner),
        Component::Metric(inner) => render_metric(inner),
        Component::Callout(inner) => render_callout(inner, ctx),
        Component::Hero(inner) => render_hero(inner, ctx),
        Component::Narrative(inner) => render_narrative(inner, ctx),
        Component::Table(inner) => render_table(inner),
        Component::Timeline(inner) => render_timeline(inner, ctx),
        Component::Disclosure(inner) => render_disclosure(inner, ctx),
    }
}

fn render_markdown(markdown: &Markdown) -> String {
    let parser = pulldown_cmark::Parser::new(&markdown.content);
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, parser);
    format!(r#"<div class="ae-markdown" data-glance-component="markdown">{html}</div>"#)
}

fn render_code(code: &Code) -> String {
    let lang_attr = code
        .language
        .as_deref()
        .map(|language| format!(r#" data-lang="{}""#, html_escape(language)))
        .unwrap_or_default();
    format!(
        r#"<pre class="ae-code" data-glance-component="code"{lang_attr}><code>{}</code></pre>"#,
        html_escape(&code.content)
    )
}

fn render_diff(diff: &Diff) -> String {
    format!(
        r#"<pre class="ae-diff" data-glance-component="diff"><code>{}</code></pre>"#,
        html_escape(&diff.unified)
    )
}

fn render_terminal(terminal: &Terminal) -> String {
    format!(
        r#"<pre class="ae-terminal" data-glance-component="terminal">{}</pre>"#,
        html_escape(&terminal.content)
    )
}

fn render_image(image: &Image) -> String {
    let caption = image
        .caption
        .as_ref()
        .map(|caption| format!("<figcaption>{}</figcaption>", html_escape(caption)))
        .unwrap_or_default();
    format!(
        r#"<figure class="ae-image" data-glance-component="image" data-asset-id="{}"><div class="ae-image-fallback" role="img" aria-label="{}">{}</div>{caption}</figure>"#,
        html_escape(&image.asset_id),
        html_escape(&image.alt),
        html_escape(&image.alt)
    )
}

fn render_mermaid(mermaid: &Mermaid) -> String {
    format!(
        r#"<pre class="ae-mermaid" data-glance-component="mermaid">{}</pre>"#,
        html_escape(&mermaid.source)
    )
}

fn render_metric(metric: &Metric) -> String {
    format!(
        r#"<span class="ae-stat-badge" data-glance-component="metric"><span class="ae-stat-value">{}</span><span class="ae-stat-label">{}</span></span>"#,
        html_escape(&metric.value),
        html_escape(&metric.label)
    )
}

fn render_callout(callout: &Callout, ctx: &RenderContext<'_>) -> String {
    format!(
        r#"<article class="ae-callout" data-glance-component="callout" data-kind="{}"><h3>{}</h3><p>{}</p></article>"#,
        callout.kind.as_str(),
        html_escape(&callout.title),
        render_inline_nodes(&callout.body, ctx.cite_href)
    )
}

fn render_hero(hero: &Hero, ctx: &RenderContext<'_>) -> String {
    let mut html = format!(
        r#"<header class="ae-hero" data-glance-component="hero"><h1 class="ae-strong">{}</h1><p class="ae-dim">{}</p>"#,
        html_escape(&hero.title),
        render_inline_nodes(&hero.summary, ctx.cite_href)
    );
    if !hero.stats.is_empty() {
        html.push_str(r#"<div class="ae-stat-badges">"#);
        for stat in &hero.stats {
            html.push_str(&format!(
                r#"<span class="ae-stat-badge"><span class="ae-stat-value">{}</span><span class="ae-stat-label">{}</span></span>"#,
                html_escape(&stat.value),
                html_escape(&stat.label)
            ));
        }
        html.push_str("</div>");
    }
    html.push_str("</header>");
    html
}

fn render_narrative(narrative: &Narrative, ctx: &RenderContext<'_>) -> String {
    let body = match &narrative.status {
        NarrativeStatus::Ok { paragraphs } => {
            crate::structural::render_narrative_paragraphs(paragraphs, ctx.cite_href)
        }
        // The fail-open reason is diagnosability detail, not reader
        // content -- it never reaches this banner (aesthetic-927 finding #6).
        NarrativeStatus::Unavailable { reason: _ } => r#"<p class="ae-dim">Narrative synthesis unavailable this run. Showing the deterministic sections below.</p>"#.to_string(),
    };
    format!(
        r#"<section class="ae-section" data-glance-component="narrative"><h2>{}</h2>{body}</section>"#,
        html_escape(&narrative.heading)
    )
}

fn render_table(table: &Table) -> String {
    if table.rows.is_empty() {
        let note = table.empty_note.as_deref().unwrap_or("");
        return format!(
            r#"<section class="ae-section" data-glance-component="table"><h2>{}</h2><p class="ae-dim">{}</p></section>"#,
            html_escape(&table.heading),
            html_escape(note)
        );
    }
    let header: String = table
        .columns
        .iter()
        .map(|column| {
            let class = if column.numeric {
                r#" class="num""#
            } else {
                ""
            };
            format!("<th{class}>{}</th>", html_escape(&column.label))
        })
        .collect();
    let rows: String = table
        .rows
        .iter()
        .map(|row| {
            let cells: String = table
                .columns
                .iter()
                .map(|column| {
                    let value = row
                        .cells
                        .iter()
                        .find(|cell| cell.column_key == column.key)
                        .map(|cell| render_cell(&cell.value))
                        .unwrap_or_default();
                    let class = if column.numeric {
                        r#" class="num""#
                    } else {
                        ""
                    };
                    format!("<td{class}>{value}</td>")
                })
                .collect();
            format!("<tr>{cells}</tr>")
        })
        .collect();
    let demoted = table
        .demoted_note
        .as_ref()
        .map(|note| format!(r#"<p class="ae-dim">{}</p>"#, html_escape(note)))
        .unwrap_or_default();
    format!(
        r#"<section class="ae-section" data-glance-component="table"><h2>{}</h2><div class="ae-plate"><table class="ae-table"><thead><tr>{header}</tr></thead><tbody>{rows}</tbody></table></div>{demoted}</section>"#,
        html_escape(&table.heading)
    )
}

fn render_timeline(timeline: &Timeline, ctx: &RenderContext<'_>) -> String {
    if timeline.entries.is_empty() {
        let note = timeline.empty_note.as_deref().unwrap_or("");
        return format!(
            r#"<section class="ae-section" data-glance-component="timeline"><h2>{}</h2><p class="ae-dim">{}</p></section>"#,
            html_escape(&timeline.heading),
            html_escape(note)
        );
    }
    let items: String = timeline
        .entries
        .iter()
        .map(|entry| {
            let body = if let Some(link) = &entry.link {
                format!(
                    r#"<a href="{}">{}</a>"#,
                    html_escape(link),
                    html_escape(&entry.summary)
                )
            } else {
                html_escape(&entry.summary)
            };
            let detail = if entry.detail.is_empty() {
                String::new()
            } else {
                format!(
                    r#"<details class="ae-trail-detail"><summary>detail</summary>{}</details>"#,
                    render_inline_nodes(&entry.detail, ctx.cite_href)
                )
            };
            format!(
                r#"<li class="ae-trail-item"><div class="ae-trail-head"><time class="ae-trail-time" datetime="{}" title="{}">{}</time><span class="ae-trail-who">{}</span></div><div class="ae-trail-body"><span class="ae-dim">{}</span> {body}</div>{detail}</li>"#,
                html_escape(&entry.at),
                html_escape(&entry.at),
                html_escape(&relative_time(&entry.at, ctx.now)),
                html_escape(&entry.actor),
                html_escape(&entry.kind)
            )
        })
        .collect();
    format!(
        r#"<section class="ae-section" data-glance-component="timeline"><h2>{}</h2><ul class="ae-trail">{items}</ul></section>"#,
        html_escape(&timeline.heading)
    )
}

fn render_disclosure(disclosure: &Disclosure, ctx: &RenderContext<'_>) -> String {
    let children: String = disclosure
        .children
        .iter()
        .map(|child| render_component(child, ctx))
        .collect();
    format!(
        r#"<details class="ae-disclosure" data-glance-component="disclosure"><summary>{}</summary><div class="ae-disclosure-inner">{children}</div></details>"#,
        html_escape(&disclosure.heading)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inline::InlineNode;
    use crate::leaf::CalloutKind;
    use crate::structural::{Cell, CellValue, ColumnSpec, Row, TimelineEntry};

    fn ctx() -> RenderContext<'static> {
        RenderContext {
            now: DateTime::parse_from_rfc3339("2026-07-05T21:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            cite_href: &|ref_id| format!("#cite-{ref_id}"),
        }
    }

    #[test]
    fn every_kind_renders_without_panicking_and_carries_its_own_marker() {
        let components = vec![
            Component::Markdown(Markdown {
                content: "hello *world*".into(),
            }),
            Component::Code(Code {
                language: Some("rust".into()),
                content: "fn main() {}".into(),
                cite_ref_id: None,
            }),
            Component::Diff(Diff {
                unified: "+added\n-removed".into(),
            }),
            Component::Terminal(Terminal {
                content: "$ ls\nfile.rs".into(),
            }),
            Component::Image(Image {
                asset_id: "sha256:abc".into(),
                alt: "a chart".into(),
                caption: Some("figure 1".into()),
            }),
            Component::Mermaid(Mermaid {
                source: "graph TD; A-->B;".into(),
            }),
            Component::Metric(Metric {
                label: "PRs".into(),
                value: "3".into(),
            }),
            Component::Callout(Callout {
                kind: CalloutKind::Hurt,
                title: "sharp edge".into(),
                body: vec![InlineNode::Text { text: "ok".into() }],
            }),
        ];
        for component in &components {
            let html = render_component(component, &ctx());
            assert!(
                html.contains(&format!(
                    r#"data-glance-component="{}""#,
                    component.kind_name()
                )),
                "missing marker for {}: {html}",
                component.kind_name()
            );
        }
    }

    #[test]
    fn hero_renders_title_summary_and_stats() {
        let hero = Component::Hero(Hero {
            title: "Fleet retro".into(),
            summary: vec![InlineNode::Text {
                text: "24h ending now".into(),
            }],
            stats: vec![Metric {
                label: "PRs".into(),
                value: "3".into(),
            }],
            image_intent: None,
        });
        let html = render_component(&hero, &ctx());
        assert!(html.contains("Fleet retro"));
        assert!(html.contains("ae-stat-badges"));
    }

    #[test]
    fn table_renders_generic_column_schema() {
        let table = Component::Table(Table {
            heading: "Repo activity".into(),
            columns: vec![
                ColumnSpec {
                    key: "repo".into(),
                    label: "repo".into(),
                    numeric: false,
                },
                ColumnSpec {
                    key: "commits".into(),
                    label: "commits".into(),
                    numeric: true,
                },
            ],
            rows: vec![Row {
                cells: vec![
                    Cell {
                        column_key: "repo".into(),
                        value: CellValue::Text {
                            text: "landmark".into(),
                        },
                    },
                    Cell {
                        column_key: "commits".into(),
                        value: CellValue::Text { text: "5".into() },
                    },
                ],
            }],
            empty_note: None,
            demoted_note: None,
        });
        let html = render_component(&table, &ctx());
        assert!(html.contains("ae-table"));
        assert!(html.contains("landmark"));
        assert!(html.contains(r#"<td class="num">5</td>"#));
    }

    #[test]
    fn timeline_renders_relative_time_not_a_raw_iso_string_as_visible_text() {
        let timeline = Component::Timeline(Timeline {
            heading: "Timeline".into(),
            entries: vec![TimelineEntry {
                at: "2026-07-05T04:25:01Z".into(),
                actor: "landmark".into(),
                kind: "pr-merged".into(),
                summary: "PR #200 merged".into(),
                link: None,
                detail: vec![],
            }],
            empty_note: None,
        });
        let html = render_component(&timeline, &ctx());
        assert!(html.contains(">16h ago<"));
        assert!(!html.contains(">2026-07-05T04:25:01Z<"));
    }

    #[test]
    fn disclosure_renders_children_recursively() {
        let disclosure = Component::Disclosure(Disclosure {
            heading: "more".into(),
            children: vec![Component::Markdown(Markdown {
                content: "detail".into(),
            })],
        });
        let html = render_component(&disclosure, &ctx());
        assert!(html.contains("<details"));
        assert!(html.contains("ae-markdown"));
    }

    #[test]
    fn escapes_untrusted_text_content() {
        let hero = Component::Hero(Hero {
            title: "<script>alert(1)</script>".into(),
            summary: vec![InlineNode::Text { text: "s".into() }],
            stats: vec![],
            image_intent: None,
        });
        let html = render_component(&hero, &ctx());
        assert!(!html.contains("<script>alert(1)</script>"));
        assert!(html.contains("&lt;script&gt;"));
    }
}

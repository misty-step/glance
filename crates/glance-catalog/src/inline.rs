//! Citation-aware rich text, generalized from glance-gen's `InlineNode`
//! (`crates/glance-gen/src/spec.rs`) -- the one piece of real, non-obvious
//! IP the sideshow research named (aesthetic-926 oracle comment): subtle
//! source-link popovers instead of visible bracket citations.
//!
//! glance cites source-code line ranges (`path:lines`, resolved to a GitHub
//! blob URL at render time using a source SHA the spec author doesn't
//! know). fleet-retro cites evidence-pack items by opaque id (resolved to a
//! local `#cite-<id>` anchor into a "cited evidence" list). Forcing both
//! into one concrete shape would be a category error -- a source line range
//! and an evidence-pack id mean different things. Instead `Cite::ref_id` is
//! an opaque string the *consumer* assigns meaning to, and href resolution
//! is a caller-supplied closure passed to `render_inline_nodes`, not a field
//! on the node. This keeps the type a model can emit without knowing the
//! render-time URL scheme, while href construction stays deterministic,
//! per-consumer code -- not a rigid shared schema for something two
//! consumers use two different ways.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum InlineNode {
    Text {
        text: String,
    },
    Link {
        text: String,
        href: String,
    },
    /// `ref_id` is opaque: glance encodes it as `"path:lines"`, fleet-retro
    /// as an evidence-pack item id. Neither the type nor this crate's
    /// renderer interprets it -- only the `cite_href` resolver passed to
    /// `render_inline_nodes` does.
    Cite {
        text: String,
        ref_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineError {
    message: String,
}

impl InlineError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for InlineError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for InlineError {}

/// A non-empty run of inline nodes with every node's required text
/// non-blank. Mirrors glance-gen's `validate_inline_nodes` -- the same rule
/// enforced in one place instead of once per consumer.
pub fn validate_inline_nodes(label: &str, nodes: &[InlineNode]) -> Result<(), InlineError> {
    if nodes.is_empty() {
        return Err(InlineError::new(format!("{label} cannot be empty")));
    }
    for node in nodes {
        match node {
            InlineNode::Text { text } => {
                if text.is_empty() {
                    return Err(InlineError::new(format!("{label} text cannot be empty")));
                }
            }
            InlineNode::Link { text, href } => {
                if text.trim().is_empty() || href.trim().is_empty() {
                    return Err(InlineError::new(format!(
                        "{label} link requires text and href"
                    )));
                }
            }
            InlineNode::Cite { text, ref_id } => {
                if text.trim().is_empty() || ref_id.trim().is_empty() {
                    return Err(InlineError::new(format!(
                        "{label} cite requires text and ref_id"
                    )));
                }
            }
        }
    }
    Ok(())
}

pub fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Configures how a `Cite` node renders: `href` resolves the opaque `ref_id`
/// to a link target (the one seam every consumer already needed). `class`
/// and `label` are the generic escape hatch a consumer opts into without any
/// glance-specific vocabulary landing in this crate: overriding `class` lets
/// a consumer's own stylesheet target its Cite anchors, and supplying
/// `label` renders a `data-cite-label` attribute (e.g. for a CSS
/// `content: attr(data-cite-label)` hover popover) -- omitted entirely when
/// `None`, so a consumer with no popover pays nothing extra.
pub struct CiteRender<'a> {
    pub href: &'a dyn Fn(&str) -> String,
    pub class: &'a str,
    pub label: Option<&'a dyn Fn(&str) -> String>,
}

impl<'a> CiteRender<'a> {
    /// The zero-config default: `ae-cite` class, no popover label.
    pub fn new(href: &'a dyn Fn(&str) -> String) -> Self {
        Self {
            href,
            class: "ae-cite",
            label: None,
        }
    }
}

/// Render a run of inline nodes to HTML using the default `Cite` styling
/// (`ae-cite`, no popover label). See `render_inline_nodes_styled` for the
/// escape hatch.
pub fn render_inline_nodes(nodes: &[InlineNode], cite_href: &dyn Fn(&str) -> String) -> String {
    render_inline_nodes_styled(nodes, &CiteRender::new(cite_href))
}

/// Render a run of inline nodes to HTML, using `cite`'s class/label
/// overrides for every `Cite` node encountered.
pub fn render_inline_nodes_styled(nodes: &[InlineNode], cite: &CiteRender<'_>) -> String {
    nodes
        .iter()
        .map(|node| render_inline_node(node, cite))
        .collect()
}

fn render_inline_node(node: &InlineNode, cite: &CiteRender<'_>) -> String {
    match node {
        InlineNode::Text { text } => html_escape(text),
        InlineNode::Link { text, href } => {
            format!(
                r#"<a href="{}">{}</a>"#,
                html_escape(href),
                html_escape(text)
            )
        }
        InlineNode::Cite { text, ref_id } => {
            let href = (cite.href)(ref_id);
            let label_attr = cite
                .label
                .map(|label| format!(r#" data-cite-label="{}""#, html_escape(&label(ref_id))))
                .unwrap_or_default();
            format!(
                r#"<a class="{}" data-glance-cite="{}"{label_attr} href="{}">{}</a>"#,
                html_escape(cite.class),
                html_escape(ref_id),
                html_escape(&href),
                html_escape(text)
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_run() {
        assert!(validate_inline_nodes("x", &[]).is_err());
    }

    #[test]
    fn rejects_blank_text() {
        let nodes = vec![InlineNode::Text {
            text: String::new(),
        }];
        assert!(validate_inline_nodes("x", &nodes).is_err());
    }

    #[test]
    fn accepts_well_formed_run() {
        let nodes = vec![
            InlineNode::Text {
                text: "see ".into(),
            },
            InlineNode::Cite {
                text: "the spec".into(),
                ref_id: "crates/glance-gen/src/spec.rs:1-10".into(),
            },
        ];
        assert!(validate_inline_nodes("x", &nodes).is_ok());
    }

    #[test]
    fn renders_cite_through_the_supplied_resolver_not_a_hardcoded_scheme() {
        let nodes = vec![InlineNode::Cite {
            text: "landmark-907".into(),
            ref_id: "card:landmark-907".into(),
        }];
        let html = render_inline_nodes(&nodes, &|ref_id| format!("#cite-{ref_id}"));
        assert!(html.contains("href=\"#cite-card:landmark-907\""));
        assert!(html.contains("data-glance-cite=\"card:landmark-907\""));
        assert!(html.contains(">landmark-907<"));
    }

    #[test]
    fn two_consumers_can_resolve_the_same_ref_id_shape_two_different_ways() {
        let source_cite = InlineNode::Cite {
            text: "spec.rs".into(),
            ref_id: "crates/glance-gen/src/spec.rs:1-10".into(),
        };
        let github_html = render_inline_nodes(std::slice::from_ref(&source_cite), &|ref_id| {
            format!("https://github.com/misty-step/glance/blob/HEAD/{ref_id}")
        });
        let local_html = render_inline_nodes(std::slice::from_ref(&source_cite), &|ref_id| {
            format!("#source-{}", ref_id.replace(['/', ':'], "-"))
        });
        assert!(github_html.contains("https://github.com/misty-step/glance/blob/HEAD/"));
        assert!(local_html.contains("#source-crates-glance-gen-src-spec.rs-1-10"));
    }

    #[test]
    fn escapes_text_content() {
        let nodes = vec![InlineNode::Text {
            text: "<script>".into(),
        }];
        let html = render_inline_nodes(&nodes, &|_| String::new());
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }
}

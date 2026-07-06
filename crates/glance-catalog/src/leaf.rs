//! Tier 1 -- leaf surfaces: content containers. Shape and naming steal
//! Sideshow (`modem-dev/sideshow`, already ported verbatim into
//! `glass/src/lib.rs::SurfaceKind`) verbatim per the aesthetic-926 oracle
//! research and team-lead ruling. These are the primitives a live stream
//! (Glass) posts with no mandated order; the structural tier
//! (`crate::structural`) is the report grammar built on top of them.

use serde::{Deserialize, Serialize};

use crate::CatalogError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Markdown {
    pub content: String,
}

impl Markdown {
    pub fn validate(&self) -> Result<(), CatalogError> {
        require_non_empty("markdown.content", &self.content)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Code {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    pub content: String,
    /// Opaque citation ref (see `crate::inline::InlineNode::Cite`) -- lets a
    /// code block itself carry a "why this is here" source pointer, the
    /// same convergence glance's citation-aware inline nodes already prove
    /// out for prose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cite_ref_id: Option<String>,
}

impl Code {
    pub fn validate(&self) -> Result<(), CatalogError> {
        require_non_empty("code.content", &self.content)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diff {
    pub unified: String,
}

impl Diff {
    pub fn validate(&self) -> Result<(), CatalogError> {
        require_non_empty("diff.unified", &self.unified)
    }
}

/// SGR-only ANSI terminal output -- not a full TUI emulator. Ship the
/// caveat rather than silently degrade escape sequences the viewer can't
/// render (oracle research, section 04).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Terminal {
    pub content: String,
}

impl Terminal {
    pub fn validate(&self) -> Result<(), CatalogError> {
        require_non_empty("terminal.content", &self.content)
    }
}

/// Content-addressed asset + caption. `asset_id` is expected to be a
/// content hash (Sideshow's SHA256-as-id dedup, called out in the research
/// as "genuinely clever, worth the port") -- this crate does not compute
/// the hash, it only carries the id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Image {
    pub asset_id: String,
    pub alt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
}

impl Image {
    pub fn validate(&self) -> Result<(), CatalogError> {
        require_non_empty("image.asset_id", &self.asset_id)?;
        require_non_empty("image.alt", &self.alt)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mermaid {
    pub source: String,
}

impl Mermaid {
    pub fn validate(&self) -> Result<(), CatalogError> {
        require_non_empty("mermaid.source", &self.source)
    }
}

/// Label + value chip. Unifies glance's `StatChip` and fleet-retro's
/// `StatCallout` -- literally the same struct, invented twice
/// (`glance-gen/src/spec.rs::StatChip`, `weave/apps/fleet-retro/src/spec.rs::StatCallout`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metric {
    pub label: String,
    pub value: String,
}

impl Metric {
    pub fn validate(&self) -> Result<(), CatalogError> {
        require_non_empty("metric.label", &self.label)?;
        require_non_empty("metric.value", &self.value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalloutKind {
    Seam,
    Hurt,
    Invariant,
    Contract,
}

impl CalloutKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Seam => "seam",
            Self::Hurt => "hurt",
            Self::Invariant => "invariant",
            Self::Contract => "contract",
        }
    }
}

/// Kind + title + cited prose. Kept as-is from glance-next's `Callouts`
/// item -- no equivalent existed in Sideshow or fleet-retro.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Callout {
    pub kind: CalloutKind,
    pub title: String,
    pub body: Vec<crate::inline::InlineNode>,
}

impl Callout {
    pub fn validate(&self) -> Result<(), CatalogError> {
        require_non_empty("callout.title", &self.title)?;
        crate::inline::validate_inline_nodes("callout.body", &self.body)
            .map_err(|error| CatalogError::new(error.to_string()))
    }
}

fn require_non_empty(label: &str, value: &str) -> Result<(), CatalogError> {
    if value.trim().is_empty() {
        return Err(CatalogError::new(format!("{label} is required")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inline::InlineNode;

    #[test]
    fn markdown_rejects_blank_content() {
        assert!(
            Markdown {
                content: "  ".into()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn image_requires_asset_id_and_alt() {
        assert!(
            Image {
                asset_id: String::new(),
                alt: "a chart".into(),
                caption: None,
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn metric_unifies_label_value_shape() {
        let metric = Metric {
            label: "PRs".into(),
            value: "3".into(),
        };
        assert!(metric.validate().is_ok());
    }

    #[test]
    fn callout_validates_its_cited_body() {
        let callout = Callout {
            kind: CalloutKind::Hurt,
            title: "sharp edge".into(),
            body: vec![InlineNode::Text { text: "ok".into() }],
        };
        assert!(callout.validate().is_ok());
        assert_eq!(callout.kind.as_str(), "hurt");
    }
}

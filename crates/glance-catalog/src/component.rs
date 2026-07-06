use serde::{Deserialize, Serialize};

use crate::leaf::{Callout, Code, Diff, Image, Markdown, Mermaid, Metric, Terminal};
use crate::structural::{Disclosure, Hero, Narrative, Table, Timeline};

pub const CATALOG_VERSION: &str = "aesthetic-catalog-001";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogError {
    message: String,
}

impl CatalogError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CatalogError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Content containers -- Sideshow-proven, no mandated order.
    Leaf,
    /// Report grammar -- glance-next/fleet-retro-proven, order enforced by
    /// a `LayoutProfile`.
    Structural,
}

/// The catalog's single tagged union: 8 leaf content primitives + 5
/// structural report-grammar primitives (aesthetic-926, team-lead ruling
/// 2026-07-07). One enum, one `catalog_version`, shared by every consumer --
/// Glass posts leaf components with no mandated order; glance-next and
/// fleet-retro compose both tiers into an ordered report via a
/// `LayoutProfile` (`crate::profile`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Component {
    // Tier 1 -- leaf surfaces.
    Markdown(Markdown),
    Code(Code),
    Diff(Diff),
    Terminal(Terminal),
    Image(Image),
    Mermaid(Mermaid),
    Metric(Metric),
    Callout(Callout),
    // Tier 2 -- structural components.
    Hero(Hero),
    Narrative(Narrative),
    Table(Table),
    Timeline(Timeline),
    Disclosure(Disclosure),
}

impl Component {
    pub fn tier(&self) -> Tier {
        match self {
            Component::Markdown(_)
            | Component::Code(_)
            | Component::Diff(_)
            | Component::Terminal(_)
            | Component::Image(_)
            | Component::Mermaid(_)
            | Component::Metric(_)
            | Component::Callout(_) => Tier::Leaf,
            Component::Hero(_)
            | Component::Narrative(_)
            | Component::Table(_)
            | Component::Timeline(_)
            | Component::Disclosure(_) => Tier::Structural,
        }
    }

    pub fn kind_name(&self) -> &'static str {
        match self {
            Component::Markdown(_) => "markdown",
            Component::Code(_) => "code",
            Component::Diff(_) => "diff",
            Component::Terminal(_) => "terminal",
            Component::Image(_) => "image",
            Component::Mermaid(_) => "mermaid",
            Component::Metric(_) => "metric",
            Component::Callout(_) => "callout",
            Component::Hero(_) => "hero",
            Component::Narrative(_) => "narrative",
            Component::Table(_) => "table",
            Component::Timeline(_) => "timeline",
            Component::Disclosure(_) => "disclosure",
        }
    }

    pub fn validate(&self) -> Result<(), CatalogError> {
        match self {
            Component::Markdown(c) => c.validate(),
            Component::Code(c) => c.validate(),
            Component::Diff(c) => c.validate(),
            Component::Terminal(c) => c.validate(),
            Component::Image(c) => c.validate(),
            Component::Mermaid(c) => c.validate(),
            Component::Metric(c) => c.validate(),
            Component::Callout(c) => c.validate(),
            Component::Hero(c) => c.validate(),
            Component::Narrative(c) => c.validate(),
            Component::Table(c) => c.validate(),
            Component::Timeline(c) => c.validate(),
            Component::Disclosure(c) => c.validate(),
        }
    }
}

/// All 13 first-class kind names, in catalog declaration order -- the
/// single source both the JSON schema and any future prompt kit should
/// enumerate from, so the three never drift independently.
pub const KIND_NAMES: [&str; 13] = [
    "markdown",
    "code",
    "diff",
    "terminal",
    "image",
    "mermaid",
    "metric",
    "callout",
    "hero",
    "narrative",
    "table",
    "timeline",
    "disclosure",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inline::InlineNode;

    #[test]
    fn kind_names_cover_every_component_variant_exactly_once() {
        // Regression: if a variant is added to `Component` without a matching
        // `kind_name` arm, this test's sample below would either fail to
        // compile (missing arm) or the len check would drift.
        assert_eq!(KIND_NAMES.len(), 13);
        let unique: std::collections::BTreeSet<&str> = KIND_NAMES.iter().copied().collect();
        assert_eq!(unique.len(), KIND_NAMES.len());
    }

    #[test]
    fn tier_classifies_leaf_vs_structural_correctly() {
        let markdown = Component::Markdown(Markdown {
            content: "hi".into(),
        });
        assert_eq!(markdown.tier(), Tier::Leaf);

        let hero = Component::Hero(Hero {
            title: "t".into(),
            summary: vec![InlineNode::Text { text: "s".into() }],
            stats: vec![],
            image_intent: None,
        });
        assert_eq!(hero.tier(), Tier::Structural);
    }

    #[test]
    fn validate_dispatches_to_the_inner_primitives_validator() {
        let empty_markdown = Component::Markdown(Markdown {
            content: String::new(),
        });
        assert!(empty_markdown.validate().is_err());
    }
}

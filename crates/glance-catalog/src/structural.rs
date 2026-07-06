//! Tier 2 -- structural components: report grammar. glance-next's `Hero`/
//! `Narrative` and fleet-retro's independently-built `Hero`/`Narrative`
//! converged on the same shape three times (Sideshow's implicit session
//! header counts as a third) -- the oracle research reads that as strong
//! evidence these are the right primitives, not a coincidence to arbitrate.
//! `FileTable`/`RepoActivityTable` merge into one generic `Table` with a
//! column schema; fleet-retro's `Timeline` and Sideshow's half-finished
//! `trace` merge into one `Timeline` with an optional expandable `detail`.

use serde::{Deserialize, Serialize};

use crate::inline::{InlineNode, render_inline_nodes, validate_inline_nodes};
use crate::leaf::Metric;
use crate::{CatalogError, Component};

/// Title + summary + 0-4 metric chips. Merges glance-next's `Hero` (title,
/// summary, stats, optional image_request) and fleet-retro's `Hero` +
/// `StatCallouts` (headline, subhead, a separate stat-chip component) into
/// one struct -- convergently invented 3x per the oracle research.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hero {
    pub title: String,
    pub summary: Vec<InlineNode>,
    #[serde(default)]
    pub stats: Vec<Metric>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_intent: Option<String>,
}

impl Hero {
    pub fn validate(&self) -> Result<(), CatalogError> {
        if self.title.trim().is_empty() {
            return Err(CatalogError::new("hero.title is required"));
        }
        validate_inline_nodes("hero.summary", &self.summary)
            .map_err(|error| CatalogError::new(error.to_string()))?;
        if self.stats.len() > 4 {
            return Err(CatalogError::new("hero.stats carries at most 4 chips"));
        }
        for stat in &self.stats {
            stat.validate()?;
        }
        Ok(())
    }
}

/// Heading + cited paragraphs, generalized to fail open: a synthesis stage
/// (e.g. fleet-retro's model-written "what mattered") may not produce a
/// narrative this run, and the catalog says so structurally instead of
/// forcing an empty `Ok` (aesthetic-927 finding #6: the fail-open reason is
/// diagnosability detail, not reader content, and stays out of the visible
/// banner).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Narrative {
    pub heading: String,
    pub status: NarrativeStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum NarrativeStatus {
    Ok { paragraphs: Vec<Vec<InlineNode>> },
    Unavailable { reason: String },
}

impl Narrative {
    pub fn validate(&self) -> Result<(), CatalogError> {
        if self.heading.trim().is_empty() {
            return Err(CatalogError::new("narrative.heading is required"));
        }
        match &self.status {
            NarrativeStatus::Ok { paragraphs } => {
                if paragraphs.is_empty() {
                    return Err(CatalogError::new("narrative.paragraphs is required"));
                }
                for paragraph in paragraphs {
                    validate_inline_nodes("narrative.paragraph", paragraph)
                        .map_err(|error| CatalogError::new(error.to_string()))?;
                }
            }
            NarrativeStatus::Unavailable { reason } => {
                if reason.trim().is_empty() {
                    return Err(CatalogError::new(
                        "narrative.status.reason is required when unavailable",
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnSpec {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub numeric: bool,
}

// Every variant is struct-like (never a bare newtype around a string/Vec)
// because serde's internally tagged representation (`tag = "type"`) cannot
// merge a "type" key into something that itself serializes as a raw JSON
// string or array -- only into a JSON object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CellValue {
    Text { text: String },
    Link { text: String, href: String },
    Code { items: Vec<String> },
    List { items: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    pub column_key: String,
    pub value: CellValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub cells: Vec<Cell>,
}

/// Generic labeled rows -- merges glance-next's `FileTable` (kind, name,
/// role, signatures, gotcha) and fleet-retro's `RepoActivityTable` (repo,
/// commits, prs, cards_touched, highlights) into one component with a
/// column schema instead of two hardcoded row shapes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Table {
    pub heading: String,
    pub columns: Vec<ColumnSpec>,
    pub rows: Vec<Row>,
    /// Rendered in place of the table when `rows` is empty -- an explicit
    /// empty state, never a silent omission (fleet-retro regression:
    /// `empty_repo_table_and_timeline_render_explicit_empty_state_not_omission`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty_note: Option<String>,
    /// Rows swept with no signal, demoted out of the table body into one
    /// muted note instead of padding the table with dead rows (fleet-retro's
    /// `quiet_repos`, designer critique finding #4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demoted_note: Option<String>,
}

impl Table {
    pub fn validate(&self) -> Result<(), CatalogError> {
        if self.heading.trim().is_empty() {
            return Err(CatalogError::new("table.heading is required"));
        }
        if self.columns.is_empty() {
            return Err(CatalogError::new("table.columns is required"));
        }
        let known_keys: std::collections::BTreeSet<&str> =
            self.columns.iter().map(|c| c.key.as_str()).collect();
        if known_keys.len() != self.columns.len() {
            return Err(CatalogError::new("table.columns keys must be unique"));
        }
        for row in &self.rows {
            let mut seen = std::collections::BTreeSet::new();
            for cell in &row.cells {
                if !known_keys.contains(cell.column_key.as_str()) {
                    return Err(CatalogError::new(format!(
                        "table row cell references undeclared column {}",
                        cell.column_key
                    )));
                }
                if !seen.insert(cell.column_key.as_str()) {
                    return Err(CatalogError::new(format!(
                        "table row has duplicate cell for column {}",
                        cell.column_key
                    )));
                }
            }
        }
        if self.rows.is_empty() && self.empty_note.is_none() {
            return Err(CatalogError::new(
                "table.empty_note is required when rows is empty",
            ));
        }
        Ok(())
    }
}

/// Ordered steps, each with a one-line summary and an optional expandable
/// `detail` -- gives fleet-retro's `Timeline` and Sideshow's half-finished
/// `trace` (flagged in its own docs as never having a finished home) one
/// shared shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timeline {
    pub heading: String,
    pub entries: Vec<TimelineEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty_note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimelineEntry {
    /// RFC3339 timestamp. Rendered as relative time ("16h ago") in visible
    /// text via `crate::time::relative_time` -- never printed raw as the
    /// visible label (aesthetic-927 finding #3).
    pub at: String,
    pub actor: String,
    pub kind: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail: Vec<InlineNode>,
}

impl Timeline {
    pub fn validate(&self) -> Result<(), CatalogError> {
        if self.heading.trim().is_empty() {
            return Err(CatalogError::new("timeline.heading is required"));
        }
        if self.entries.is_empty() && self.empty_note.is_none() {
            return Err(CatalogError::new(
                "timeline.empty_note is required when entries is empty",
            ));
        }
        for entry in &self.entries {
            if entry.at.trim().is_empty() {
                return Err(CatalogError::new("timeline entry.at is required"));
            }
            if entry.summary.trim().is_empty() {
                return Err(CatalogError::new("timeline entry.summary is required"));
            }
            if !entry.detail.is_empty() {
                validate_inline_nodes("timeline entry.detail", &entry.detail)
                    .map_err(|error| CatalogError::new(error.to_string()))?;
            }
        }
        Ok(())
    }
}

/// Collapsed section wrapper for report-length content. Kept as-is from
/// glance-next's `Disclosure` -- children cannot contain a nested `Hero` or
/// `Disclosure`, same rule as `validate_disclosure_child`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Disclosure {
    pub heading: String,
    pub children: Vec<Component>,
}

impl Disclosure {
    pub fn validate(&self) -> Result<(), CatalogError> {
        if self.heading.trim().is_empty() {
            return Err(CatalogError::new("disclosure.heading is required"));
        }
        for child in &self.children {
            if matches!(child, Component::Hero(_) | Component::Disclosure(_)) {
                return Err(CatalogError::new(
                    "disclosure children cannot contain hero or nested disclosure",
                ));
            }
            child.validate()?;
        }
        Ok(())
    }
}

/// Render a table cell to HTML. `cite_href` is threaded through even though
/// no cell variant carries an `InlineNode::Cite` today, so a future
/// `Code`/`List` citation extension doesn't need a new render entry point.
pub fn render_cell(value: &CellValue) -> String {
    match value {
        CellValue::Text { text } => crate::inline::html_escape(text),
        CellValue::Link { text, href } => format!(
            r#"<a href="{}">{}</a>"#,
            crate::inline::html_escape(href),
            crate::inline::html_escape(text)
        ),
        CellValue::Code { items } => items
            .iter()
            .map(|item| format!("<code>{}</code>", crate::inline::html_escape(item)))
            .collect(),
        CellValue::List { items } => crate::inline::html_escape(&items.join("; ")),
    }
}

pub fn render_narrative_paragraphs(
    paragraphs: &[Vec<InlineNode>],
    cite_href: &dyn Fn(&str) -> String,
) -> String {
    paragraphs
        .iter()
        .map(|paragraph| format!("<p>{}</p>", render_inline_nodes(paragraph, cite_href)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Vec<InlineNode> {
        vec![InlineNode::Text { text: s.into() }]
    }

    #[test]
    fn hero_merges_summary_and_stats_in_one_struct() {
        let hero = Hero {
            title: "Fleet retro".into(),
            summary: text("24h ending now"),
            stats: vec![Metric {
                label: "PRs".into(),
                value: "3".into(),
            }],
            image_intent: None,
        };
        assert!(hero.validate().is_ok());
    }

    #[test]
    fn hero_rejects_more_than_four_stats() {
        let hero = Hero {
            title: "t".into(),
            summary: text("s"),
            stats: (0..5)
                .map(|i| Metric {
                    label: i.to_string(),
                    value: i.to_string(),
                })
                .collect(),
            image_intent: None,
        };
        assert!(hero.validate().is_err());
    }

    #[test]
    fn narrative_unavailable_requires_a_reason_but_hides_it_from_the_type_signature_only_not_validation()
     {
        let narrative = Narrative {
            heading: "What mattered".into(),
            status: NarrativeStatus::Unavailable {
                reason: String::new(),
            },
        };
        assert!(narrative.validate().is_err());
    }

    #[test]
    fn table_rejects_cell_referencing_undeclared_column() {
        let table = Table {
            heading: "Repo activity".into(),
            columns: vec![ColumnSpec {
                key: "repo".into(),
                label: "repo".into(),
                numeric: false,
            }],
            rows: vec![Row {
                cells: vec![Cell {
                    column_key: "commits".into(),
                    value: CellValue::Text { text: "5".into() },
                }],
            }],
            empty_note: None,
            demoted_note: None,
        };
        assert!(table.validate().is_err());
    }

    #[test]
    fn table_requires_empty_note_when_rows_is_empty() {
        let table = Table {
            heading: "Repo activity".into(),
            columns: vec![ColumnSpec {
                key: "repo".into(),
                label: "repo".into(),
                numeric: false,
            }],
            rows: vec![],
            empty_note: None,
            demoted_note: None,
        };
        assert!(table.validate().is_err());
        let with_note = Table {
            empty_note: Some("No repo activity in this window.".into()),
            ..table
        };
        assert!(with_note.validate().is_ok());
    }

    #[test]
    fn table_generalizes_file_table_and_repo_activity_table_shapes() {
        // FileTable-shaped columns.
        let file_table = Table {
            heading: "Files to know".into(),
            columns: vec![
                ColumnSpec {
                    key: "kind".into(),
                    label: "kind".into(),
                    numeric: false,
                },
                ColumnSpec {
                    key: "name".into(),
                    label: "name".into(),
                    numeric: false,
                },
            ],
            rows: vec![Row {
                cells: vec![
                    Cell {
                        column_key: "kind".into(),
                        value: CellValue::Text {
                            text: "file".into(),
                        },
                    },
                    Cell {
                        column_key: "name".into(),
                        value: CellValue::Text {
                            text: "spec.rs".into(),
                        },
                    },
                ],
            }],
            empty_note: None,
            demoted_note: None,
        };
        assert!(file_table.validate().is_ok());

        // RepoActivityTable-shaped columns, same struct.
        let repo_table = Table {
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
            demoted_note: Some("2 repo(s) swept with no activity: glass, canary".into()),
        };
        assert!(repo_table.validate().is_ok());
    }

    #[test]
    fn timeline_requires_empty_note_when_entries_is_empty() {
        let timeline = Timeline {
            heading: "Timeline".into(),
            entries: vec![],
            empty_note: None,
        };
        assert!(timeline.validate().is_err());
    }

    #[test]
    fn timeline_entry_supports_optional_expandable_detail() {
        let timeline = Timeline {
            heading: "Timeline".into(),
            entries: vec![TimelineEntry {
                at: "2026-07-05T04:25:01Z".into(),
                actor: "landmark".into(),
                kind: "pr-merged".into(),
                summary: "PR #200 merged".into(),
                link: Some("https://github.com/misty-step/landmark/pull/200".into()),
                detail: text("full trace detail"),
            }],
            empty_note: None,
        };
        assert!(timeline.validate().is_ok());
    }

    #[test]
    fn disclosure_rejects_nested_hero_and_nested_disclosure() {
        let with_hero = Disclosure {
            heading: "More".into(),
            children: vec![Component::Hero(Hero {
                title: "t".into(),
                summary: text("s"),
                stats: vec![],
                image_intent: None,
            })],
        };
        assert!(with_hero.validate().is_err());

        let with_nested_disclosure = Disclosure {
            heading: "More".into(),
            children: vec![Component::Disclosure(Disclosure {
                heading: "Even more".into(),
                children: vec![],
            })],
        };
        assert!(with_nested_disclosure.validate().is_err());
    }
}

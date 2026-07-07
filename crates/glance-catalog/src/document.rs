//! The producer seam's wire envelope (glance-929): `{catalog_version,
//! title?, layout_profile?, components}` -- the shape `catalog.schema.json`'s
//! top-level object already declares, given a Rust type and one render
//! entry point so a non-Rust producer's JSON has exactly one thing to
//! target (`crates/glance-catalog/src/bin/glance-catalog.rs` is the CLI that
//! reads this JSON and writes the rendered HTML).

use serde::Deserialize;

use crate::CatalogError;
use crate::component::{CATALOG_VERSION, Component};
use crate::inline::html_escape;
use crate::profile::{LayoutProfile, REPORT, STREAM, validate_layout};
use crate::render::{RenderContext, render_component};

/// Wire name for a `crate::profile::LayoutProfile` -- the profile itself
/// isn't `Deserialize` (it's two bools plus a `&'static str` name, not a
/// value a producer should be able to forge arbitrarily), so a document
/// picks one of the crate's two shipped profiles by name instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutProfileName {
    Stream,
    Report,
}

impl LayoutProfileName {
    pub fn profile(self) -> LayoutProfile {
        match self {
            Self::Stream => STREAM,
            Self::Report => REPORT,
        }
    }
}

impl Default for LayoutProfileName {
    /// Most producers compose a synthesized report (hero-first, progressive
    /// disclosure) rather than a chronological stream -- see
    /// `crate::profile`'s own doc comment for the distinction.
    fn default() -> Self {
        Self::Report
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentError {
    message: String,
}

impl DocumentError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for DocumentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for DocumentError {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogDocument {
    pub catalog_version: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub layout_profile: LayoutProfileName,
    pub components: Vec<Component>,
}

impl CatalogDocument {
    /// Parses and version-checks a document. Layout/component validation is
    /// a separate step (`validate`) so a caller can distinguish "malformed
    /// JSON or wrong catalog_version" from "well-formed but invalid layout"
    /// -- the two failure shapes the CLI seam documents separately.
    pub fn from_json(json: &str) -> Result<Self, DocumentError> {
        let document: Self = serde_json::from_str(json)
            .map_err(|error| DocumentError::new(format!("invalid catalog document: {error}")))?;
        if document.catalog_version != CATALOG_VERSION {
            return Err(DocumentError::new(format!(
                "catalog_version must be {CATALOG_VERSION}, got {}",
                document.catalog_version
            )));
        }
        Ok(document)
    }

    pub fn validate(&self) -> Result<(), CatalogError> {
        validate_layout(&self.components, &self.layout_profile.profile())
    }

    /// Renders a complete, self-contained (structurally -- no stylesheet is
    /// inlined, matching this crate's own `examples/exemplar.rs` precedent)
    /// HTML document. Callers that want citation verification run it against
    /// this output (see `glance_check::CitationChecker::check_citations`),
    /// not against the JSON.
    pub fn render(&self, ctx: &RenderContext<'_>) -> String {
        let title = self.title.as_deref().unwrap_or("Glance Catalog Document");
        let body: String = self
            .components
            .iter()
            .map(|component| render_component(component, ctx))
            .collect();
        format!(
            "<!doctype html>\n<html lang=\"en\" data-glance-catalog-version=\"{}\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{}</title>\n</head>\n<body>\n{}\n</body>\n</html>\n",
            html_escape(CATALOG_VERSION),
            html_escape(title),
            body
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};

    fn ctx() -> RenderContext<'static> {
        RenderContext {
            now: DateTime::parse_from_rfc3339("2026-07-07T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            cite_href: &|ref_id| format!("#cite-{ref_id}"),
            cite_class: None,
            cite_label: None,
        }
    }

    fn valid_json() -> String {
        format!(
            r#"{{"catalog_version":"{CATALOG_VERSION}","title":"Demo","components":[{{"type":"hero","title":"t","summary":[{{"type":"text","text":"s"}}]}},{{"type":"markdown","content":"hi"}}]}}"#
        )
    }

    #[test]
    fn from_json_rejects_wrong_catalog_version() {
        let json = r#"{"catalog_version":"nope","components":[]}"#;
        let error = CatalogDocument::from_json(json).expect_err("wrong version");
        assert!(error.to_string().contains("catalog_version"));
    }

    #[test]
    fn from_json_rejects_malformed_json() {
        let error = CatalogDocument::from_json("not json").expect_err("malformed");
        assert!(error.to_string().contains("invalid catalog document"));
    }

    #[test]
    fn from_json_defaults_layout_profile_to_report() {
        let document = CatalogDocument::from_json(&valid_json()).expect("valid document");
        assert_eq!(document.layout_profile, LayoutProfileName::Report);
    }

    #[test]
    fn validate_enforces_the_chosen_layout_profile() {
        let document = CatalogDocument::from_json(&valid_json()).expect("valid document");
        assert!(document.validate().is_ok());

        let stream_json = valid_json().replace(
            "\"components\"",
            "\"layout_profile\":\"stream\",\"components\"",
        );
        let stream_document =
            CatalogDocument::from_json(&stream_json).expect("valid stream document");
        assert_eq!(stream_document.layout_profile, LayoutProfileName::Stream);
        assert!(stream_document.validate().is_ok());

        // report requires hero-first; a stream-labeled document with the same
        // components validates under STREAM but would fail under REPORT if a
        // second hero were added -- prove the profile selection is live, not
        // ignored, with a document REPORT rejects.
        let two_heroes = format!(
            r#"{{"catalog_version":"{CATALOG_VERSION}","components":[{{"type":"hero","title":"a","summary":[{{"type":"text","text":"s"}}]}},{{"type":"hero","title":"b","summary":[{{"type":"text","text":"s"}}]}}]}}"#
        );
        let report_document = CatalogDocument::from_json(&two_heroes).expect("valid json");
        assert!(report_document.validate().is_err());
    }

    #[test]
    fn render_produces_a_self_contained_document_with_every_component() {
        let document = CatalogDocument::from_json(&valid_json()).expect("valid document");
        let html = document.render(&ctx());
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<title>Demo</title>"));
        assert!(html.contains("data-glance-component=\"hero\""));
        assert!(html.contains("data-glance-component=\"markdown\""));
    }

    #[test]
    fn render_falls_back_to_a_generic_title_when_none_is_given() {
        let json = format!(
            r#"{{"catalog_version":"{CATALOG_VERSION}","components":[{{"type":"markdown","content":"hi"}}]}}"#
        );
        let document = CatalogDocument::from_json(&json).expect("valid document");
        let html = document.render(&ctx());
        assert!(html.contains("<title>Glance Catalog Document</title>"));
    }
}

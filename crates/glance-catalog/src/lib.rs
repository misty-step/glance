//! glance-catalog -- the shared, two-tier report component catalog
//! (aesthetic-926): 8 leaf content primitives (Sideshow-proven, ported
//! verbatim into `glass/src/lib.rs::SurfaceKind`) + 5 structural
//! report-grammar primitives (glance-next/fleet-retro-proven, converged on
//! independently three times), one `Component` tagged union, one
//! `CATALOG_VERSION`, and per-consumer `LayoutProfile`s instead of a
//! hardcoded single-page-doc ordering rule.
//!
//! This crate declares the catalog and renders it; it does not decide what
//! order a spec should compose it in for glance-next's own doc pages (that
//! retargeting is glance-922's job) or how a model should be prompted to
//! emit conformant JSON (a separate prompt-kit child). fleet-retro
//! (`weave/apps/fleet-retro`) is this pass's proof consumer: its
//! `RetroSpec` becomes a `LayoutProfile::REPORT` composition over this
//! catalog's structural components, plus a small set of retro-specific
//! extension sections (`Footer`, `Receipts`, `Provenance`) that stayed
//! local because they are report-specific diagnostics/evidence sections,
//! not primitives another consumer converged on independently -- forcing
//! them into the shared catalog would be arbitrating a winner where no
//! second implementation exists to merge with.

pub mod component;
pub mod document;
pub mod inline;
pub mod leaf;
pub mod profile;
pub mod render;
pub mod structural;
pub mod time;

pub use component::{CATALOG_VERSION, CatalogError, Component, KIND_NAMES, Tier};
pub use document::{CatalogDocument, DocumentError, LayoutProfileName};
pub use inline::InlineNode;
pub use profile::{LayoutProfile, REPORT, STREAM, validate_layout};
pub use render::{RenderContext, render_component};
pub use time::relative_time;

/// Hand-authored JSON Schema for the catalog envelope + all 13 primitive
/// kinds, in the same style as glance-gen's `catalog.schema.json`
/// (`crates/glance-gen/catalog/catalog.schema.json`) -- one `$defs` entry
/// per kind, `additionalProperties: false` throughout so an unrecognized
/// field fails schema validation instead of round-tripping silently.
pub const CATALOG_SCHEMA_JSON: &str = include_str!("../catalog/catalog.schema.json");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_schema_is_well_formed_json() {
        let parsed: serde_json::Value =
            serde_json::from_str(CATALOG_SCHEMA_JSON).expect("catalog.schema.json must parse");
        assert_eq!(parsed["title"], "Aesthetic Report Component Catalog v001");
    }

    #[test]
    fn every_kind_name_has_a_schema_def() {
        let parsed: serde_json::Value =
            serde_json::from_str(CATALOG_SCHEMA_JSON).expect("catalog.schema.json must parse");
        for kind in KIND_NAMES {
            assert!(
                parsed["$defs"].get(kind).is_some(),
                "catalog.schema.json is missing a $defs entry for kind {kind}"
            );
        }
    }
}

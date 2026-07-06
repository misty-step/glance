//! Per-consumer layout profiles, generalizing glance-gen's
//! `PageSpec::validate_for_kind` (`crates/glance-gen/src/spec.rs`) ordering
//! rules -- hero-first, disclosure-last -- from a single hardcoded
//! single-page-doc consumer into a profile parameter. Oracle research
//! recommendation, ruled binding by the team lead 2026-07-07: "same enum,
//! same validator function signature, different profile passed in -- this
//! is a small, contained change... not a rewrite."
//!
//! Glass's live stream needs no mandated order: posts arrive chronologically,
//! each one a bag of leaf surfaces, no hero/table required. A synthesized
//! report (fleet-retro, glance-next docs) keeps strict progressive
//! disclosure. `STREAM` and `REPORT` are the two profiles this crate ships;
//! a future consumer with a third ordering need constructs its own
//! `LayoutProfile` value rather than this crate growing a bespoke enum
//! variant per consumer.

use crate::{CatalogError, Component};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutProfile {
    pub name: &'static str,
    /// If true, exactly one `Component::Hero` must be present and it must
    /// be the first component.
    pub hero_first: bool,
    /// If true, once a `Component::Disclosure` appears, every remaining
    /// component must also be a `Disclosure` -- progressive disclosure,
    /// last in reading order.
    pub disclosure_last: bool,
}

/// Chronological live-stream posting (Glass): no mandated order, any mix of
/// leaf surfaces, zero or many heroes over the session's lifetime.
pub const STREAM: LayoutProfile = LayoutProfile {
    name: "stream",
    hero_first: false,
    disclosure_last: false,
};

/// Single synthesized report, produced once or on a cadence (fleet-retro,
/// glance-next docs): strict progressive-disclosure order.
pub const REPORT: LayoutProfile = LayoutProfile {
    name: "report",
    hero_first: true,
    disclosure_last: true,
};

/// Validate every component individually, then check the profile's ordering
/// rules across the whole sequence.
pub fn validate_layout(
    components: &[Component],
    profile: &LayoutProfile,
) -> Result<(), CatalogError> {
    for component in components {
        component.validate()?;
    }

    if profile.hero_first {
        let hero_positions: Vec<usize> = components
            .iter()
            .enumerate()
            .filter(|(_, component)| matches!(component, Component::Hero(_)))
            .map(|(index, _)| index)
            .collect();
        match hero_positions.as_slice() {
            [] => {
                return Err(CatalogError::new(format!(
                    "profile {} requires exactly one hero, first",
                    profile.name
                )));
            }
            [0] => {}
            [_] => {
                return Err(CatalogError::new(format!(
                    "profile {} requires hero to be the first component",
                    profile.name
                )));
            }
            _ => {
                return Err(CatalogError::new(format!(
                    "profile {} allows hero only once",
                    profile.name
                )));
            }
        }
    }

    if profile.disclosure_last {
        let mut seen_disclosure = false;
        for component in components {
            if seen_disclosure && !matches!(component, Component::Disclosure(_)) {
                return Err(CatalogError::new(format!(
                    "profile {} requires disclosure components last",
                    profile.name
                )));
            }
            if matches!(component, Component::Disclosure(_)) {
                seen_disclosure = true;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inline::InlineNode;
    use crate::leaf::Markdown;
    use crate::structural::{Disclosure, Hero};

    fn hero() -> Component {
        Component::Hero(Hero {
            title: "t".into(),
            summary: vec![InlineNode::Text { text: "s".into() }],
            stats: vec![],
            image_intent: None,
        })
    }

    fn markdown() -> Component {
        Component::Markdown(Markdown {
            content: "hi".into(),
        })
    }

    fn disclosure() -> Component {
        Component::Disclosure(Disclosure {
            heading: "more".into(),
            children: vec![],
        })
    }

    #[test]
    fn stream_profile_allows_no_hero_and_any_order() {
        let components = vec![markdown(), markdown()];
        assert!(validate_layout(&components, &STREAM).is_ok());
    }

    #[test]
    fn stream_profile_allows_multiple_heroes_one_per_post() {
        let components = vec![hero(), markdown(), hero()];
        assert!(validate_layout(&components, &STREAM).is_ok());
    }

    #[test]
    fn report_profile_requires_hero_first() {
        let missing = vec![markdown()];
        assert!(validate_layout(&missing, &REPORT).is_err());

        let wrong_position = vec![markdown(), hero()];
        assert!(validate_layout(&wrong_position, &REPORT).is_err());

        let correct = vec![hero(), markdown()];
        assert!(validate_layout(&correct, &REPORT).is_ok());
    }

    #[test]
    fn report_profile_rejects_a_second_hero() {
        let components = vec![hero(), hero()];
        assert!(validate_layout(&components, &REPORT).is_err());
    }

    #[test]
    fn report_profile_requires_disclosure_last() {
        let out_of_order = vec![hero(), disclosure(), markdown()];
        assert!(validate_layout(&out_of_order, &REPORT).is_err());

        let in_order = vec![hero(), markdown(), disclosure(), disclosure()];
        assert!(validate_layout(&in_order, &REPORT).is_ok());
    }

    #[test]
    fn every_component_is_individually_validated_even_under_stream() {
        let invalid = vec![Component::Markdown(Markdown {
            content: String::new(),
        })];
        assert!(validate_layout(&invalid, &STREAM).is_err());
    }
}

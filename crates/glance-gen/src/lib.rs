use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelTier {
    Cheap,
    Mid,
    Frontier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKind {
    Leaf,
    Interior,
    Root,
    CrossCutting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierRouter {
    pub leaf: ModelTier,
    pub interior: ModelTier,
    pub root: ModelTier,
}

impl Default for TierRouter {
    fn default() -> Self {
        Self {
            leaf: ModelTier::Cheap,
            interior: ModelTier::Mid,
            root: ModelTier::Frontier,
        }
    }
}

impl TierRouter {
    pub fn tier_for(&self, kind: PageKind) -> ModelTier {
        match kind {
            PageKind::Leaf => self.leaf,
            PageKind::Interior => self.interior,
            PageKind::Root | PageKind::CrossCutting => self.root,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationRequest {
    pub directory: PathBuf,
    pub source_sha: String,
    pub kind: PageKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedPage {
    pub html: String,
    pub tier: ModelTier,
    pub spend_micros: u64,
}

#[derive(Debug, Error)]
pub enum GenerationError {
    #[error("generation provider is scaffold-only: {0}")]
    ScaffoldOnly(String),
}

pub trait PageGenerator {
    fn generate(&self, request: GenerationRequest) -> Result<GeneratedPage, GenerationError>;
}

#[derive(Debug, Clone, Default)]
pub struct MockProvider {
    router: TierRouter,
}

impl MockProvider {
    pub fn new(router: TierRouter) -> Self {
        Self { router }
    }
}

impl PageGenerator for MockProvider {
    fn generate(&self, request: GenerationRequest) -> Result<GeneratedPage, GenerationError> {
        let tier = self.router.tier_for(request.kind);
        let directory = request.directory.display();
        Ok(GeneratedPage {
            html: format!(
                "<!doctype html><html data-source-sha=\"{}\"><body><h1>{directory}</h1><p data-glance-cite=\"README.md:1\">Mock glance page.</p></body></html>",
                request.source_sha
            ),
            tier,
            spend_micros: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_page_kinds_to_model_tiers() {
        let router = TierRouter::default();
        assert_eq!(router.tier_for(PageKind::Leaf), ModelTier::Cheap);
        assert_eq!(router.tier_for(PageKind::Interior), ModelTier::Mid);
        assert_eq!(router.tier_for(PageKind::Root), ModelTier::Frontier);
    }
}

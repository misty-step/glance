//! glance-gen's page spec: a directory-tour envelope (breadcrumbs,
//! parent/child/sibling links, source-line citations resolved to GitHub blob
//! URLs) that validates through `glance_catalog`'s shared primitives
//! (glance-927, first slice of glance-922).
//!
//! Wire JSON is unchanged (`catalog.schema.json` still governs `path`/`lines`
//! citations and the fixed `kind`/`title`/`body` callout shape --
//! `crates/glance/src/main.rs` depends on this schema, a real external
//! contract). `Hero` and `Callout` field validation delegates to
//! `glance_catalog::structural`/`leaf` via small `to_catalog_*` converters;
//! `hero.stats`' 2-4 cap is glance's own display-band rule, layered on top
//! the same way `validate_for_kind` layers ordering rules on
//! `glance_catalog::validate_layout`. `Narrative`'s own validate body stays
//! (heading/paragraphs non-empty is cheaper to check directly than to
//! convert into `glance_catalog::structural::Narrative`'s
//! `NarrativeStatus::Ok` wrapper for).
//!
//! Rendering stays local for Hero, Narrative, Callouts, the file table, and
//! Disclosure -- not merely to avoid a rewrite, but because
//! `glance_catalog::render_component` cannot reproduce three things this
//! crate's own tests and downstream readers depend on: (1) hero's and
//! callouts' `data-glance-section` markers, read back by
//! `context.rs::distill_generated_page` to assemble a parent page's prompt
//! context from an already-generated child; (2) the citation popover's
//! `data-cite-label` attribute and `glance-cite` class (`kit.css`), which the
//! shared crate's `Cite` renderer does not emit; (3) file-table directory
//! cells rendered as `glance_check::directory_href` links, which need this
//! crate's own `RenderContext` (`DirectorySnapshot` + current directory).
//! `FileTable`/`FileRow` stay glance's own types rather than `Table`'s
//! generic column/cell schema for the same reason -- the fixed
//! kind/name/role/signatures/gotcha shape plus directory links and per-row
//! citations don't fit without inventing shared-crate surface only this one
//! consumer would use (the "arbitrating a winner where no second
//! implementation exists" trap the crate's own docs warn about).
//! `FlowDiagram`/`ImageFigure`/`CustomHtml` have no catalog equivalent at
//! all and stay fully local, per the same principle.
//!
//! Deleted: this module's own HTML-escaping (`glance_catalog::inline::html_escape`
//! is now the sole `html_escape`), and `Hero`/`Callout`'s own
//! field-validation bodies where `glance_catalog` already proves the same
//! rule once.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use glance_core::DirectorySnapshot;
use serde::{Deserialize, Serialize};

use crate::{GenerationError, PageKind};

pub const CATALOG_VERSION: &str = "glance-catalog-001";
pub const CATALOG_SCHEMA_JSON: &str = include_str!("../catalog/catalog.schema.json");
pub const CATALOG_PROMPT_MD: &str = include_str!("../catalog/catalog.md");

const KIT_CSS: &str = include_str!("../../../assets/kit.css");
const KIT_JS: &str = include_str!("../../../assets/kit.js");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageSpec {
    pub catalog_version: String,
    pub title: String,
    pub components: Vec<Component>,
}

impl PageSpec {
    pub fn validate_for_kind(&self, kind: PageKind) -> Result<(), SpecError> {
        if self.catalog_version != CATALOG_VERSION {
            return Err(SpecError::new(format!(
                "catalog_version must be {CATALOG_VERSION}, got {}",
                self.catalog_version
            )));
        }
        if self.title.trim().is_empty() {
            return Err(SpecError::new("title is required"));
        }
        let Some(first) = self.components.first() else {
            return Err(SpecError::new(
                "components must include hero and file_table",
            ));
        };
        if !matches!(first, Component::Hero(_)) {
            return Err(SpecError::new("hero must be the first component"));
        }

        let mut file_table_index = None;
        let mut first_story_index = None;
        let mut seen_disclosure = false;
        let mut seen_file_table = false;
        let mut custom_html_count = 0;
        for (index, component) in self.components.iter().enumerate() {
            if seen_disclosure && !matches!(component, Component::Disclosure(_)) {
                return Err(SpecError::new(
                    "disclosure components must be last in progressive order",
                ));
            }
            if seen_file_table && !matches!(component, Component::Disclosure(_)) {
                return Err(SpecError::new(
                    "file_table must follow all story components and precede disclosures",
                ));
            }
            match component {
                Component::Hero(hero) => {
                    if index != 0 {
                        return Err(SpecError::new("hero may appear only once, first"));
                    }
                    hero.validate()?;
                    if hero.image_request.is_some()
                        && !matches!(kind, PageKind::Root | PageKind::CrossCutting)
                    {
                        return Err(SpecError::new(
                            "hero image_request is allowed only on root pages",
                        ));
                    }
                }
                Component::Narrative(narrative) => {
                    first_story_index.get_or_insert(index);
                    narrative.validate()?;
                }
                Component::FlowDiagram(flow) => {
                    first_story_index.get_or_insert(index);
                    flow.validate()?;
                }
                Component::FileTable(table) => {
                    if file_table_index.replace(index).is_some() {
                        return Err(SpecError::new("file_table may appear only once"));
                    }
                    seen_file_table = true;
                    table.validate()?;
                }
                Component::Callouts(callouts) => {
                    first_story_index.get_or_insert(index);
                    callouts.validate()?;
                }
                Component::Disclosure(disclosure) => {
                    seen_disclosure = true;
                    custom_html_count += disclosure.validate(kind)?;
                }
                Component::ImageFigure(figure) => {
                    first_story_index.get_or_insert(index);
                    if kind == PageKind::Leaf {
                        return Err(SpecError::new("leaf pages cannot request images"));
                    }
                    figure.image_request.validate()?;
                }
                Component::CustomHtml(custom) => {
                    first_story_index.get_or_insert(index);
                    custom_html_count += 1;
                    if kind == PageKind::Leaf {
                        return Err(SpecError::new("custom_html is not allowed on leaf pages"));
                    }
                    custom.validate()?;
                }
            }
        }

        let Some(file_table_index) = file_table_index else {
            return Err(SpecError::new("file_table is required on every page"));
        };
        let Some(first_story_index) = first_story_index else {
            return Err(SpecError::new(
                "narrative, flow_diagram, callouts, image_figure, or custom_html must appear before file_table",
            ));
        };
        if !matches!(
            self.components.get(1),
            Some(Component::Narrative(_) | Component::FlowDiagram(_))
        ) {
            return Err(SpecError::new("narrative or flow_diagram must follow hero"));
        }
        if file_table_index <= first_story_index {
            return Err(SpecError::new(
                "file_table must follow narrative or flow content",
            ));
        }
        if custom_html_count > 1 {
            return Err(SpecError::new("custom_html budget is max 1 per page"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Component {
    Hero(Hero),
    Narrative(Narrative),
    FlowDiagram(FlowDiagram),
    FileTable(FileTable),
    Callouts(Callouts),
    Disclosure(Disclosure),
    ImageFigure(ImageFigure),
    CustomHtml(CustomHtml),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hero {
    pub title: String,
    pub summary: Vec<InlineNode>,
    pub stats: Vec<StatChip>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_request: Option<ImageRequestSpec>,
}

impl Hero {
    fn validate(&self) -> Result<(), SpecError> {
        // Field-level checks (title non-empty, summary well-formed) delegate
        // to glance_catalog::structural::Hero -- the same rule, one place.
        // `hero.stats` 2-4 cap is glance's own display-band choice
        // (glance_catalog's Hero deliberately has no upper bound, see that
        // crate's doc comment), so it's layered on top here, not inside the
        // shared validator.
        to_catalog_hero(self)
            .validate()
            .map_err(|error| SpecError::new(error.to_string()))?;
        if !(2..=4).contains(&self.stats.len()) {
            return Err(SpecError::new("hero.stats must contain 2-4 stat chips"));
        }
        if let Some(request) = &self.image_request {
            request.validate()?;
        }
        Ok(())
    }
}

fn to_catalog_hero(hero: &Hero) -> glance_catalog::structural::Hero {
    glance_catalog::structural::Hero {
        title: hero.title.clone(),
        summary: to_catalog_inline_nodes(&hero.summary),
        stats: hero.stats.iter().map(to_catalog_metric).collect(),
        image_intent: None,
    }
}

fn to_catalog_metric(stat: &StatChip) -> glance_catalog::leaf::Metric {
    glance_catalog::leaf::Metric {
        label: stat.label.clone(),
        value: stat.value.clone(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatChip {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Narrative {
    pub heading: String,
    pub paragraphs: Vec<Vec<InlineNode>>,
}

impl Narrative {
    fn validate(&self) -> Result<(), SpecError> {
        if self.heading.trim().is_empty() {
            return Err(SpecError::new("narrative.heading is required"));
        }
        if self.paragraphs.is_empty() {
            return Err(SpecError::new("narrative.paragraphs is required"));
        }
        for paragraph in &self.paragraphs {
            validate_inline_nodes("narrative.paragraph", paragraph)?;
        }
        Ok(())
    }
}

/// Wire shape stays `path` + `lines` (see the module doc comment); converts
/// to `glance_catalog::InlineNode` for validation only, via
/// `to_catalog_inline_nodes`. Rendering stays local (`render_inline_nodes`
/// below) so the citation popover keeps its `data-cite-label` attribute and
/// `glance-cite` class, which `glance_catalog`'s `Cite` renderer doesn't
/// produce.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum InlineNode {
    Text {
        text: String,
    },
    Cite {
        text: String,
        path: String,
        lines: String,
    },
    Link {
        text: String,
        href: String,
    },
}

fn to_catalog_inline_node(node: &InlineNode) -> glance_catalog::InlineNode {
    match node {
        InlineNode::Text { text } => glance_catalog::InlineNode::Text { text: text.clone() },
        InlineNode::Link { text, href } => glance_catalog::InlineNode::Link {
            text: text.clone(),
            href: href.clone(),
        },
        InlineNode::Cite { text, path, lines } => glance_catalog::InlineNode::Cite {
            text: text.clone(),
            ref_id: format!("{path}:{lines}"),
        },
    }
}

fn to_catalog_inline_nodes(nodes: &[InlineNode]) -> Vec<glance_catalog::InlineNode> {
    nodes.iter().map(to_catalog_inline_node).collect()
}

fn render_inline_nodes(nodes: &[InlineNode], context: &RenderContext<'_>) -> String {
    nodes
        .iter()
        .map(|node| render_inline_node(node, context))
        .collect()
}

fn render_inline_node(node: &InlineNode, context: &RenderContext<'_>) -> String {
    match node {
        InlineNode::Text { text } => html_escape(text),
        InlineNode::Link { text, href } => format!(
            r#"<a href="{}">{}</a>"#,
            html_escape(href),
            html_escape(text)
        ),
        InlineNode::Cite { text, path, lines } => {
            let raw = format!("{path}:{lines}");
            let href = source_href(context, path, lines);
            format!(
                r#"<a class="glance-cite" data-glance-cite="{}" data-cite-label="{}" href="{}">{}</a>"#,
                html_escape(&raw),
                html_escape(&raw),
                html_escape(&href),
                html_escape(text)
            )
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowDiagram {
    pub nodes: Vec<FlowNode>,
    pub edges: Vec<FlowEdge>,
    #[serde(default)]
    pub lanes: Vec<String>,
}

impl FlowDiagram {
    fn validate(&self) -> Result<(), SpecError> {
        if self.nodes.is_empty() {
            return Err(SpecError::new("flow_diagram.nodes is required"));
        }
        let ids = self
            .nodes
            .iter()
            .map(|node| node.id.as_str())
            .collect::<BTreeSet<_>>();
        if ids.len() != self.nodes.len() {
            return Err(SpecError::new("flow_diagram node ids must be unique"));
        }
        for node in &self.nodes {
            if node.id.trim().is_empty()
                || node.label.trim().is_empty()
                || node.kind.trim().is_empty()
            {
                return Err(SpecError::new(
                    "flow_diagram nodes require id, label, and kind",
                ));
            }
        }
        for edge in &self.edges {
            if !ids.contains(edge.from.as_str()) || !ids.contains(edge.to.as_str()) {
                return Err(SpecError::new(
                    "flow_diagram edges must reference declared nodes",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowNode {
    pub id: String,
    pub label: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowEdge {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Kept as glance-gen's own type rather than mapped onto
/// `glance_catalog::structural::Table` -- see the module doc comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileTable {
    pub rows: Vec<FileRow>,
}

impl FileTable {
    fn validate(&self) -> Result<(), SpecError> {
        for row in &self.rows {
            if row.name.trim().is_empty() {
                return Err(SpecError::new("file_table row name is required"));
            }
            if row.role.trim().is_empty() {
                return Err(SpecError::new(format!(
                    "file_table role for {} is required",
                    row.name
                )));
            }
            if row.role.split_whitespace().count() > 12 {
                return Err(SpecError::new(format!(
                    "file_table role for {} exceeds 12 words",
                    row.name
                )));
            }
            if let Some(cite) = &row.cite {
                cite.validate()?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRow {
    pub name: String,
    pub kind: FileRowKind,
    pub role: String,
    pub signatures: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gotcha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cite: Option<CitationRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileRowKind {
    File,
    Dir,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Callouts {
    pub items: Vec<Callout>,
}

impl Callouts {
    fn validate(&self) -> Result<(), SpecError> {
        if self.items.is_empty() {
            return Err(SpecError::new("callouts.items is required"));
        }
        for item in &self.items {
            item.validate()?;
        }
        Ok(())
    }
}

/// Wire shape is glance-gen's own (`kind`/`title`/`body`); field validation
/// delegates to `glance_catalog::leaf::Callout` via `to_catalog_callout`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Callout {
    pub kind: CalloutKind,
    pub title: String,
    pub body: Vec<InlineNode>,
}

impl Callout {
    fn validate(&self) -> Result<(), SpecError> {
        to_catalog_callout(self)
            .validate()
            .map_err(|error| SpecError::new(error.to_string()))
    }
}

fn to_catalog_callout(callout: &Callout) -> glance_catalog::leaf::Callout {
    glance_catalog::leaf::Callout {
        kind: to_catalog_callout_kind(callout.kind),
        title: callout.title.clone(),
        body: to_catalog_inline_nodes(&callout.body),
    }
}

fn to_catalog_callout_kind(kind: CalloutKind) -> glance_catalog::leaf::CalloutKind {
    match kind {
        CalloutKind::Seam => glance_catalog::leaf::CalloutKind::Seam,
        CalloutKind::Hurt => glance_catalog::leaf::CalloutKind::Hurt,
        CalloutKind::Invariant => glance_catalog::leaf::CalloutKind::Invariant,
        CalloutKind::Contract => glance_catalog::leaf::CalloutKind::Contract,
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
    fn as_str(self) -> &'static str {
        match self {
            Self::Seam => "seam",
            Self::Hurt => "hurt",
            Self::Invariant => "invariant",
            Self::Contract => "contract",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Disclosure {
    pub heading: String,
    pub children: Vec<Component>,
}

impl Disclosure {
    fn validate(&self, kind: PageKind) -> Result<usize, SpecError> {
        if self.heading.trim().is_empty() {
            return Err(SpecError::new("disclosure.heading is required"));
        }
        let mut custom_html_count = 0;
        for child in &self.children {
            custom_html_count += validate_disclosure_child(child, kind)?;
        }
        Ok(custom_html_count)
    }
}

fn validate_disclosure_child(component: &Component, kind: PageKind) -> Result<usize, SpecError> {
    match component {
        Component::Hero(_) | Component::Disclosure(_) => Err(SpecError::new(
            "disclosure children cannot contain hero or nested disclosure",
        )),
        Component::Narrative(narrative) => {
            narrative.validate()?;
            Ok(0)
        }
        Component::FlowDiagram(flow) => {
            flow.validate()?;
            Ok(0)
        }
        Component::FileTable(table) => {
            table.validate()?;
            Ok(0)
        }
        Component::Callouts(callouts) => {
            callouts.validate()?;
            Ok(0)
        }
        Component::ImageFigure(figure) => {
            if kind == PageKind::Leaf {
                return Err(SpecError::new("leaf pages cannot request images"));
            }
            figure.image_request.validate()?;
            Ok(0)
        }
        Component::CustomHtml(custom) => {
            if kind == PageKind::Leaf {
                return Err(SpecError::new("custom_html is not allowed on leaf pages"));
            }
            custom.validate()?;
            Ok(1)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageFigure {
    pub image_request: ImageRequestSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageRequestSpec {
    pub intent: String,
    #[serde(default)]
    pub emphasis: Vec<String>,
}

impl ImageRequestSpec {
    fn validate(&self) -> Result<(), SpecError> {
        if self.intent.trim().is_empty() {
            return Err(SpecError::new("image_request.intent is required"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomHtml {
    pub title: String,
    pub html: String,
    #[serde(default)]
    pub citations: Vec<CitationRef>,
}

impl CustomHtml {
    fn validate(&self) -> Result<(), SpecError> {
        if self.title.trim().is_empty() {
            return Err(SpecError::new("custom_html.title is required"));
        }
        for citation in &self.citations {
            citation.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CitationRef {
    pub path: String,
    pub lines: String,
}

impl CitationRef {
    fn raw(&self) -> String {
        format!("{}:{}", self.path, self.lines)
    }

    fn validate(&self) -> Result<(), SpecError> {
        validate_citation(&self.path, &self.lines)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecError {
    message: String,
}

impl SpecError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for SpecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SpecError {}

pub struct RenderContext<'a> {
    pub snapshot: &'a DirectorySnapshot,
    pub directory: &'a Path,
    pub source_sha: &'a str,
    pub prompt_version: &'a str,
    pub kind: PageKind,
}

pub fn render_page_spec(
    spec: &PageSpec,
    context: &RenderContext<'_>,
) -> Result<String, GenerationError> {
    spec.validate_for_kind(context.kind)
        .map_err(|error| GenerationError::InvalidSpec {
            message: error.to_string(),
        })?;

    let title = if spec.title.trim().is_empty() {
        path_label(context.directory)
    } else {
        spec.title.clone()
    };
    let flow_edges = spec_flow_edges(spec);
    let mut body = String::new();
    body.push_str(r#"<div class="glance-shell">"#);
    body.push_str(&render_topbar(&title, context));
    body.push_str(r#"<main class="glance-main">"#);
    for component in &spec.components {
        body.push_str(&render_component(component, context, false, &flow_edges));
    }
    body.push_str("</main></div>");

    Ok(format!(
        r#"<!doctype html>
<html lang="en" data-glance-catalog-version="{}" data-source-sha="{}" data-prompt-version="{}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{}</title>
<style>{}</style>
<script>try{{var m=localStorage.getItem('glance-theme');if(m==='light'||m==='dark'){{document.documentElement.setAttribute('data-theme',m);}}}}catch(_){{}}</script>
</head>
<body class="glance-page" data-glance-directory="{}">
{}
<div class="glance-citation-popover" aria-hidden="true"></div>
<script>{}</script>
</body>
</html>"#,
        html_escape(CATALOG_VERSION),
        html_escape(context.source_sha),
        html_escape(context.prompt_version),
        html_escape(&title),
        KIT_CSS,
        html_escape(&path_label(context.directory)),
        body,
        KIT_JS
    ))
}

fn render_topbar(title: &str, context: &RenderContext<'_>) -> String {
    format!(
        r#"<header class="glance-topbar"><nav class="glance-nav" aria-label="Glance navigation">{}</nav>{}</header>"#,
        render_navigation(title, context),
        render_theme_toggle()
    )
}

fn render_theme_toggle() -> String {
    r#"<div class="glance-theme" role="group" aria-label="Theme"><button type="button" data-theme-choice="light" aria-pressed="false">light</button><button type="button" data-theme-choice="dark" aria-pressed="false">dark</button><button type="button" data-theme-choice="system" aria-pressed="true">system</button></div>"#.to_owned()
}

fn render_navigation(title: &str, context: &RenderContext<'_>) -> String {
    let mut html = String::new();
    html.push_str(r#"<div class="glance-breadcrumb">"#);
    let crumbs = breadcrumb_dirs(context.directory);
    for (index, crumb) in crumbs.iter().enumerate() {
        if index > 0 {
            html.push_str("<span>/</span>");
        }
        let label = if crumb == Path::new(".") {
            "root".to_owned()
        } else {
            crumb
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(title)
                .to_owned()
        };
        html.push_str(&format!(
            r#"<a href="{}">{}</a>"#,
            html_escape(&glance_check::directory_href(context.directory, crumb)),
            html_escape(&label)
        ));
    }
    html.push_str("</div>");

    let mut relation_links = Vec::new();
    if context.directory != Path::new(".") {
        let parent = parent_directory(context.directory);
        relation_links.push(format!(
            r#"<a class="glance-parent-link" href="{}">parent</a>"#,
            html_escape(&glance_check::directory_href(context.directory, &parent))
        ));
    }
    if let Some(record) = context.snapshot.directory(context.directory) {
        for child in &record.child_dirs {
            relation_links.push(format!(
                r#"<a class="glance-child-link" href="{}">{}</a>"#,
                html_escape(&glance_check::directory_href(context.directory, child)),
                html_escape(&path_label(child))
            ));
        }
    }
    for sibling in sibling_dirs(context.snapshot, context.directory) {
        relation_links.push(format!(
            r#"<a class="glance-sibling-link" href="{}">{}</a>"#,
            html_escape(&glance_check::directory_href(context.directory, &sibling)),
            html_escape(&path_label(&sibling))
        ));
    }
    if !relation_links.is_empty() {
        html.push_str(r#"<div class="glance-nav-row">"#);
        html.push_str(&relation_links.join(""));
        html.push_str("</div>");
    }
    html
}

fn render_component(
    component: &Component,
    context: &RenderContext<'_>,
    nested: bool,
    flow_edges: &str,
) -> String {
    match component {
        Component::Hero(hero) => render_hero(hero, context, flow_edges),
        Component::Narrative(narrative) => render_narrative(narrative, context),
        Component::FlowDiagram(flow) => render_flow_diagram(flow),
        Component::FileTable(table) => render_file_table(table, context),
        Component::Callouts(callouts) => render_callouts(callouts, context),
        Component::Disclosure(disclosure) => render_disclosure(disclosure, context),
        Component::ImageFigure(figure) => {
            render_image_figure(&figure.image_request, context, flow_edges)
        }
        Component::CustomHtml(custom) => render_custom_html(custom, nested),
    }
}

/// Rendering stays local -- see the module doc comment; field validation
/// still delegates to `glance_catalog::structural::Hero::validate` via
/// `to_catalog_hero`.
fn render_hero(hero: &Hero, context: &RenderContext<'_>, flow_edges: &str) -> String {
    let mut html = format!(
        r#"<section class="glance-component glance-hero" data-glance-component="hero" data-glance-section="what-this-is"><div class="glance-kicker">{}</div><h1>{}</h1><p class="glance-hero-summary">{}"#,
        html_escape(CATALOG_VERSION),
        html_escape(&hero.title),
        render_inline_nodes(&hero.summary, context)
    );
    html.push_str("</p>");
    if !hero.stats.is_empty() {
        html.push_str(r#"<div class="glance-stat-band">"#);
        for stat in &hero.stats {
            html.push_str(&format!(
                r#"<div class="glance-stat"><strong>{}</strong><span>{}</span></div>"#,
                html_escape(&stat.value),
                html_escape(&stat.label)
            ));
        }
        html.push_str("</div>");
    }
    if let Some(request) = &hero.image_request {
        html.push_str(&render_image_request_figure(request, context, flow_edges));
    }
    html.push_str("</section>");
    html
}

/// Rendering stays local to preserve `data-glance-section="role-in-the-whole"`
/// -- unread by `distill_generated_page` today, kept for symmetry rather than
/// silently dropping a marker some other reader may depend on.
fn render_narrative(narrative: &Narrative, context: &RenderContext<'_>) -> String {
    let mut html = format!(
        r#"<section class="glance-component glance-narrative" data-glance-component="narrative" data-glance-section="role-in-the-whole"><h2 class="glance-section-title">{}</h2>"#,
        html_escape(&narrative.heading)
    );
    for paragraph in &narrative.paragraphs {
        html.push_str("<p>");
        html.push_str(&render_inline_nodes(paragraph, context));
        html.push_str("</p>");
    }
    html.push_str("</section>");
    html
}

/// Rough monospace glyph width at the 13px flow-diagram label font
/// (`.glance-flow-diagram text` in kit.css), used only to detect and stagger
/// overlapping edge labels — not for pixel-perfect measurement.
const FLOW_LABEL_CHAR_WIDTH_PX: i32 = 8;
/// Minimum horizontal gap required between two edge labels before they're
/// considered non-overlapping.
const FLOW_LABEL_GAP_PX: i32 = 12;
/// Vertical spacing between stacked label lanes.
const FLOW_LABEL_LANE_HEIGHT_PX: i32 = 16;
const FLOW_LABEL_BASE_Y: i32 = 52;

fn render_flow_diagram(flow: &FlowDiagram) -> String {
    let width = (flow.nodes.len().max(2) as i32 * 180).max(360);
    let mut positions = BTreeMap::new();
    for (index, node) in flow.nodes.iter().enumerate() {
        positions.insert(node.id.as_str(), (90 + index as i32 * 180, 86));
    }

    let mut edges = Vec::new();
    for edge in &flow.edges {
        if let (Some(&(from_x, from_y)), Some(&(to_x, _to_y))) = (
            positions.get(edge.from.as_str()),
            positions.get(edge.to.as_str()),
        ) {
            let mid_x = (from_x + to_x) / 2;
            let label = edge.label.as_deref().unwrap_or("");
            let half_width = (label.chars().count() as i32 * FLOW_LABEL_CHAR_WIDTH_PX) / 2;
            edges.push((from_x, from_y, to_x, mid_x, half_width, label));
        }
    }
    let lanes = assign_label_lanes(&edges);
    let top_offset = lanes.iter().copied().max().unwrap_or(0) * FLOW_LABEL_LANE_HEIGHT_PX;
    let height = 172 + top_offset;

    let mut svg = format!(
        r#"<section class="glance-component glance-flow-section" data-glance-component="flow_diagram"><h2 class="glance-section-title">Flow</h2><svg class="glance-flow-diagram" viewBox="0 0 {width} {height}" role="img" aria-label="Glance flow diagram"><defs><marker id="glance-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 Z"></path></marker></defs>"#
    );
    for ((from_x, from_y, to_x, mid_x, _half_width, label), lane) in edges.iter().zip(&lanes) {
        let label_y = FLOW_LABEL_BASE_Y + top_offset - lane * FLOW_LABEL_LANE_HEIGHT_PX;
        svg.push_str(&format!(
            r#"<path class="glance-flow-pulse" d="M{} {} H{}" marker-end="url(#glance-arrow)"></path><text x="{mid_x}" y="{label_y}">{}</text>"#,
            from_x + 58,
            from_y + top_offset,
            to_x - 58,
            html_escape(label)
        ));
    }
    for node in &flow.nodes {
        if let Some(&(x, y)) = positions.get(node.id.as_str()) {
            let y = y + top_offset;
            svg.push_str(&format!(
                r#"<g><rect x="{}" y="{}" width="116" height="48"></rect><text x="{}" y="{}">{}</text><text x="{}" y="{}">{}</text></g>"#,
                x - 58,
                y - 24,
                x,
                y,
                html_escape(&node.label),
                x,
                y + 16,
                html_escape(&node.kind)
            ));
        }
    }
    svg.push_str("</svg></section>");
    svg
}

/// Assigns each edge label a lane number (0 = the default baseline row,
/// 1, 2, ... stacked progressively higher) so that no two labels whose
/// estimated horizontal extents overlap share a lane. Diagrams with no
/// crowding get every label in lane 0, rendering exactly as before.
fn assign_label_lanes(edges: &[(i32, i32, i32, i32, i32, &str)]) -> Vec<i32> {
    let mut order: Vec<usize> = (0..edges.len()).collect();
    order.sort_by_key(|&index| {
        let (_, _, _, mid_x, half_width, _) = edges[index];
        mid_x - half_width
    });

    let mut lane_rightmost: Vec<i32> = Vec::new();
    let mut lanes = vec![0; edges.len()];
    for index in order {
        let (_, _, _, mid_x, half_width, _) = edges[index];
        let left = mid_x - half_width;
        let right = mid_x + half_width;
        let lane = lane_rightmost
            .iter()
            .position(|&rightmost| rightmost + FLOW_LABEL_GAP_PX <= left)
            .unwrap_or(lane_rightmost.len());
        if lane == lane_rightmost.len() {
            lane_rightmost.push(right);
        } else {
            lane_rightmost[lane] = right;
        }
        lanes[index] = lane as i32;
    }
    lanes
}

/// Rendering stays local -- see the module doc comment.
fn render_file_table(table: &FileTable, context: &RenderContext<'_>) -> String {
    let mut html = String::from(
        r#"<section class="glance-component glance-file-table-section" data-glance-component="file_table" data-glance-section="composition"><h2 class="glance-section-title">Files to know</h2><div class="glance-table-wrap"><table class="glance-file-table"><tr><th class="kind">kind</th><th>name</th><th>role</th><th>signatures</th><th>gotcha</th></tr>"#,
    );
    for row in &table.rows {
        let cite_attr = row
            .cite
            .as_ref()
            .map(|cite| format!(r#" data-glance-cite="{}""#, html_escape(&cite.raw())))
            .unwrap_or_default();
        html.push_str(&format!(
            r#"<tr{cite_attr}><td class="kind">{}</td><td>{}</td><td>{}</td><td><div class="glance-signatures">{} </div></td><td>{}</td></tr>"#,
            match row.kind {
                FileRowKind::File => "file",
                FileRowKind::Dir => "dir",
            },
            render_file_name(row, context),
            html_escape(&row.role),
            render_signatures(&row.signatures),
            row.gotcha
                .as_ref()
                .map(|gotcha| html_escape(gotcha))
                .unwrap_or_else(|| "none".to_owned())
        ));
    }
    html.push_str("</table></div></section>");
    html
}

fn render_file_name(row: &FileRow, context: &RenderContext<'_>) -> String {
    if row.kind == FileRowKind::Dir {
        let target = PathBuf::from(&row.name);
        format!(
            r#"<a href="{}">{}</a>"#,
            html_escape(&glance_check::directory_href(context.directory, &target)),
            html_escape(&row.name)
        )
    } else {
        html_escape(&row.name)
    }
}

fn render_signatures(signatures: &[String]) -> String {
    if signatures.is_empty() {
        return r#"<span class="glance-muted">none</span>"#.to_owned();
    }
    signatures
        .iter()
        .map(|signature| format!("<code>{}</code>", html_escape(signature)))
        .collect::<Vec<_>>()
        .join("")
}

/// Rendering stays local -- see the module doc comment; title/body still
/// validate through `glance_catalog::leaf::Callout::validate` (see
/// `to_catalog_callout`).
fn render_callouts(callouts: &Callouts, context: &RenderContext<'_>) -> String {
    let mut html = String::from(
        r#"<section class="glance-component glance-callouts" data-glance-component="callouts"><h2 class="glance-section-title">Seams and sharp edges</h2><div class="glance-callout-grid">"#,
    );
    for item in &callouts.items {
        let section = match item.kind {
            CalloutKind::Hurt => "where-it-can-hurt-you",
            CalloutKind::Seam | CalloutKind::Contract => "seams-contracts",
            CalloutKind::Invariant => "role-in-the-whole",
        };
        html.push_str(&format!(
            r#"<article class="glance-callout" data-kind="{}" data-glance-section="{section}"><h3>{}</h3><p>{}</p></article>"#,
            item.kind.as_str(),
            html_escape(&item.title),
            render_inline_nodes(&item.body, context)
        ));
    }
    html.push_str("</div></section>");
    html
}

fn render_disclosure(disclosure: &Disclosure, context: &RenderContext<'_>) -> String {
    let mut html = format!(
        r#"<details class="glance-disclosure" data-glance-component="disclosure"><summary>{}</summary><div class="glance-disclosure-inner">"#,
        html_escape(&disclosure.heading)
    );
    for child in &disclosure.children {
        html.push_str(&render_component(child, context, true, ""));
    }
    html.push_str("</div></details>");
    html
}

fn render_image_figure(
    request: &ImageRequestSpec,
    context: &RenderContext<'_>,
    flow_edges: &str,
) -> String {
    format!(
        r#"<section class="glance-component" data-glance-component="image_figure">{}</section>"#,
        render_image_request_figure(request, context, flow_edges)
    )
}

fn render_image_request_figure(
    request: &ImageRequestSpec,
    context: &RenderContext<'_>,
    flow_edges: &str,
) -> String {
    let prompt = compose_image_prompt(request, context, flow_edges);
    format!(
        r#"<figure class="glance-image-request" data-glance-image-prompt="{}" data-glance-image-alt="{}"><div class="glance-image-fallback" role="img" aria-label="{}">{}</div></figure>"#,
        html_escape(&prompt),
        html_escape(&request.intent),
        html_escape(&request.intent),
        html_escape(&request.intent)
    )
}

fn render_custom_html(custom: &CustomHtml, _nested: bool) -> String {
    let mut citation_markers = String::new();
    for citation in &custom.citations {
        citation_markers.push_str(&format!(
            r#"<span hidden data-glance-cite="{}"></span>"#,
            html_escape(&citation.raw())
        ));
    }
    format!(
        r#"<section class="glance-component" data-glance-component="custom_html"><h2 class="glance-section-title">{}</h2><iframe class="glance-custom-frame" sandbox="allow-scripts" srcdoc="{}"></iframe>{citation_markers}</section>"#,
        html_escape(&custom.title),
        html_escape(&custom.html)
    )
}

fn compose_image_prompt(
    request: &ImageRequestSpec,
    context: &RenderContext<'_>,
    flow_edges: &str,
) -> String {
    let top_level_dirs = context
        .snapshot
        .directory(Path::new("."))
        .map(|root| {
            root.child_dirs
                .iter()
                .map(|path| path_label(path))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
        .join(", ");
    let edges = if flow_edges.trim().is_empty() {
        context
            .snapshot
            .directory(context.directory)
            .map(|record| {
                if record.child_dirs.is_empty() {
                    format!("{} -> generated site", path_label(context.directory))
                } else {
                    record
                        .child_dirs
                        .iter()
                        .map(|child| {
                            format!("{} -> {}", path_label(context.directory), path_label(child))
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            })
            .unwrap_or_else(|| "source -> generated site".to_owned())
    } else {
        flow_edges.to_owned()
    };
    let emphasis = if request.emphasis.is_empty() {
        "none".to_owned()
    } else {
        request.emphasis.join(", ")
    };
    format!(
        "intent: {}; emphasis: {}; top-level dirs: {}; edges: {}; clean labeled architecture illustration, Misty Step palette, no decorative clutter",
        request.intent, emphasis, top_level_dirs, edges
    )
}

fn spec_flow_edges(spec: &PageSpec) -> String {
    let node_labels = spec
        .components
        .iter()
        .find_map(|component| match component {
            Component::FlowDiagram(flow) => Some(
                flow.nodes
                    .iter()
                    .map(|node| (node.id.as_str(), node.label.as_str()))
                    .collect::<BTreeMap<_, _>>(),
            ),
            _ => None,
        })
        .unwrap_or_default();
    spec.components
        .iter()
        .find_map(|component| match component {
            Component::FlowDiagram(flow) => Some(
                flow.edges
                    .iter()
                    .map(|edge| {
                        let from = node_labels
                            .get(edge.from.as_str())
                            .copied()
                            .unwrap_or(edge.from.as_str());
                        let to = node_labels
                            .get(edge.to.as_str())
                            .copied()
                            .unwrap_or(edge.to.as_str());
                        format!("{from} -> {to}")
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

fn validate_inline_nodes(label: &str, nodes: &[InlineNode]) -> Result<(), SpecError> {
    glance_catalog::inline::validate_inline_nodes(label, &to_catalog_inline_nodes(nodes))
        .map_err(|error| SpecError::new(error.to_string()))?;
    // glance_catalog's validator treats every Cite's ref_id as an opaque
    // string; glance's own citation-range grammar (`path:start[-end][,...]`)
    // is checked separately here, same as before.
    for node in nodes {
        if let InlineNode::Cite { path, lines, .. } = node {
            validate_citation(path, lines)?;
        }
    }
    Ok(())
}

fn validate_citation(path: &str, lines: &str) -> Result<(), SpecError> {
    if path.trim().is_empty() || lines.trim().is_empty() {
        return Err(SpecError::new("citation path and lines are required"));
    }
    for range in lines.split(',').map(str::trim) {
        if range.is_empty() {
            return Err(SpecError::new("citation ranges cannot be empty"));
        }
        glance_check::Citation::parse(&format!("{path}:{range}"))
            .map_err(|error| SpecError::new(error.to_string()))?;
    }
    Ok(())
}

fn source_href(context: &RenderContext<'_>, path: &str, lines: &str) -> String {
    let line_fragment = source_line_fragment(lines);
    if let Some(base) = github_blob_base(&context.snapshot.source_root, context.source_sha) {
        format!("{base}/{}{}", path.trim_start_matches('/'), line_fragment)
    } else {
        format!("#source-{}", slugify(&format!("{path}:{lines}")))
    }
}

fn github_blob_base(source_root: &Path, source_sha: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(source_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let remote = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let repo = remote
        .strip_prefix("git@github.com:")
        .or_else(|| remote.strip_prefix("https://github.com/"))?
        .trim_end_matches(".git")
        .trim_matches('/');
    Some(format!("https://github.com/{repo}/blob/{source_sha}"))
}

fn source_line_fragment(lines: &str) -> String {
    let first = lines.split(',').next().unwrap_or(lines);
    let (start, end) = first.split_once('-').unwrap_or((first, first));
    if start == end {
        format!("#L{start}")
    } else {
        format!("#L{start}-L{end}")
    }
}

fn breadcrumb_dirs(directory: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from(".")];
    if directory == Path::new(".") {
        return dirs;
    }
    let mut current = PathBuf::new();
    for component in directory.components() {
        if let std::path::Component::Normal(part) = component {
            current.push(part);
            dirs.push(current.clone());
        }
    }
    dirs
}

fn parent_directory(directory: &Path) -> PathBuf {
    directory
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn sibling_dirs(snapshot: &DirectorySnapshot, directory: &Path) -> Vec<PathBuf> {
    if directory == Path::new(".") {
        return Vec::new();
    }
    let parent = directory
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut siblings = snapshot
        .directory(parent)
        .map(|record| {
            record
                .child_dirs
                .iter()
                .filter(|child| child.as_path() != directory)
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    siblings.sort();
    siblings
}

fn path_label(path: &Path) -> String {
    if path == Path::new(".") {
        ".".to_owned()
    } else {
        path.components()
            .filter_map(|component| match component {
                std::path::Component::Normal(part) => Some(part.to_string_lossy()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/")
    }
}

fn slugify(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

fn html_escape(value: &str) -> String {
    glance_catalog::inline::html_escape(value)
}

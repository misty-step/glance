use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use glance_core::DirectorySnapshot;
use scraper::{Html, Selector};

use crate::{GenerationError, GenerationRequest, PageKind, ProviderOutput};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptContext {
    pub prompt: String,
    pub prompt_version: String,
    pub estimated_input_tokens: u64,
    pub metadata_notes: Vec<String>,
    pub primary_citation: Option<String>,
}

impl PromptContext {
    pub fn from_request(
        request: &GenerationRequest,
        max_file_bytes: usize,
    ) -> Result<Self, GenerationError> {
        if let Some(prompt_context) = &request.prompt_context {
            return Ok(prompt_context.clone());
        }
        let snapshot = glance_core::snapshot_tree(&request.source_root, request.source_sha.clone())
            .map_err(|error| GenerationError::Context {
                message: error.to_string(),
            })?;
        assemble_prompt_context(
            &snapshot,
            &request.directory,
            request.kind,
            max_file_bytes,
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
    }

    pub fn from_directory(
        source_root: &Path,
        directory: &Path,
        max_file_bytes: usize,
    ) -> Result<Self, GenerationError> {
        let snapshot = glance_core::snapshot_tree(source_root, "WORKTREE").map_err(|error| {
            GenerationError::Context {
                message: error.to_string(),
            }
        })?;
        let kind = snapshot
            .directory(directory)
            .map(|record| {
                if directory == Path::new(".") {
                    PageKind::Root
                } else if record.child_dirs.is_empty() {
                    PageKind::Leaf
                } else {
                    PageKind::Interior
                }
            })
            .unwrap_or(PageKind::Leaf);
        assemble_prompt_context(
            &snapshot,
            directory,
            kind,
            max_file_bytes,
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
    }

    pub(crate) fn with_retry_feedback(&self, error: &str) -> Self {
        let mut prompt = self.prompt.clone();
        prompt.push_str("\n\n# Previous output was rejected\n");
        prompt.push_str("The previous HTML failed deterministic validation: ");
        prompt.push_str(error);
        prompt.push_str("\nReturn a complete HTML document and fix only the validation error.\n");
        Self {
            prompt,
            prompt_version: self.prompt_version.clone(),
            estimated_input_tokens: self.estimated_input_tokens,
            metadata_notes: self.metadata_notes.clone(),
            primary_citation: self.primary_citation.clone(),
        }
    }
}

pub fn assemble_prompt_context(
    snapshot: &DirectorySnapshot,
    directory: &Path,
    kind: PageKind,
    max_file_bytes: usize,
    generated_pages: &BTreeMap<PathBuf, String>,
    existing_pages: &BTreeMap<PathBuf, String>,
) -> Result<PromptContext, GenerationError> {
    let directory = normalize_directory(directory);
    let template = PromptTemplate::for_kind(kind)?;
    let mut packet = ContextPacket::new(snapshot, directory.clone(), kind);
    packet.add_tier_context(max_file_bytes, generated_pages, existing_pages)?;
    let context_packet = packet.render();
    let prompt = template.render(&context_packet, kind);
    Ok(PromptContext {
        estimated_input_tokens: estimate_tokens(&prompt),
        prompt,
        prompt_version: template.version,
        metadata_notes: packet.metadata_notes,
        primary_citation: packet.primary_citation,
    })
}

struct PromptTemplate {
    version: String,
    body: String,
}

impl PromptTemplate {
    fn for_kind(kind: PageKind) -> Result<Self, GenerationError> {
        match kind {
            PageKind::Leaf => {
                Self::parse("prompts/leaf.md", include_str!("../../../prompts/leaf.md"))
            }
            PageKind::Interior => Self::parse(
                "prompts/interior.md",
                include_str!("../../../prompts/interior.md"),
            ),
            PageKind::Root | PageKind::CrossCutting => {
                Self::parse("prompts/root.md", include_str!("../../../prompts/root.md"))
            }
        }
    }

    fn parse(path: &'static str, raw: &str) -> Result<Self, GenerationError> {
        let mut lines = raw.lines();
        if lines.next() != Some("---") {
            return Err(GenerationError::PromptTemplate {
                path,
                message: "missing front matter".to_owned(),
            });
        }

        let mut version = None;
        let mut body_lines = Vec::new();
        let mut in_front_matter = true;
        for line in lines {
            if in_front_matter {
                if line == "---" {
                    in_front_matter = false;
                    continue;
                }
                if let Some((key, value)) = line.split_once(':')
                    && key.trim() == "prompt_version"
                {
                    version = Some(value.trim().to_owned());
                }
            } else {
                body_lines.push(line);
            }
        }

        let version = version.ok_or_else(|| GenerationError::PromptTemplate {
            path,
            message: "missing prompt_version".to_owned(),
        })?;

        Ok(Self {
            version,
            body: body_lines.join("\n"),
        })
    }

    fn render(&self, context_packet: &str, kind: PageKind) -> String {
        self.body
            .replace("{{context_packet}}", context_packet)
            .replace("{{prompt_version}}", &self.version)
            .replace("{{tier_name}}", kind.label())
    }
}

impl PageKind {
    fn label(self) -> &'static str {
        match self {
            PageKind::Leaf => "leaf",
            PageKind::Interior => "interior",
            PageKind::Root => "root",
            PageKind::CrossCutting => "root",
        }
    }
}

struct ContextPacket<'a> {
    snapshot: &'a DirectorySnapshot,
    directory: PathBuf,
    kind: PageKind,
    sections: Vec<String>,
    metadata_notes: Vec<String>,
    primary_citation: Option<String>,
}

impl<'a> ContextPacket<'a> {
    fn new(snapshot: &'a DirectorySnapshot, directory: PathBuf, kind: PageKind) -> Self {
        Self {
            snapshot,
            directory,
            kind,
            sections: Vec::new(),
            metadata_notes: Vec::new(),
            primary_citation: None,
        }
    }

    fn add_tier_context(
        &mut self,
        max_file_bytes: usize,
        generated_pages: &BTreeMap<PathBuf, String>,
        existing_pages: &BTreeMap<PathBuf, String>,
    ) -> Result<(), GenerationError> {
        self.add_repository_section();
        match self.kind {
            PageKind::Leaf => {
                self.add_local_files_section(max_file_bytes, LocalFileMode::Direct)?;
                self.add_parent_chain_section(generated_pages, existing_pages);
                self.add_sibling_names_section();
                self.add_empty_directory_note();
            }
            PageKind::Interior => {
                self.add_local_files_section(max_file_bytes, LocalFileMode::Direct)?;
                self.add_child_pages_section(
                    max_file_bytes,
                    ChildPageMode::Direct,
                    generated_pages,
                )?;
                self.add_parent_chain_section(generated_pages, existing_pages);
                self.add_empty_directory_note();
            }
            PageKind::Root | PageKind::CrossCutting => {
                self.add_root_metadata_section(max_file_bytes)?;
                self.add_child_pages_section(max_file_bytes, ChildPageMode::All, generated_pages)?;
                self.sections.push(
                    "## Root-only obligations\n- flows: trace 2-4 primary user or data flows across directories, each hop cited\n- data model: separate stored shapes from derived shapes\n- failure-edge index: deduplicate every child where-it-can-hurt-you section and cite it\n".to_owned(),
                );
            }
        }
        Ok(())
    }

    fn add_repository_section(&mut self) {
        let one_liner = repo_one_liner(&self.snapshot.source_root)
            .unwrap_or_else(|| self.snapshot.source_root.display().to_string());
        self.sections.push(format!(
            "## Repository\n- one_liner: {one_liner}\n- source_sha: {}\n- directory: {}\n- kind: {}\n",
            self.snapshot.source_sha,
            path_display(&self.directory),
            self.kind.label()
        ));
    }

    fn add_local_files_section(
        &mut self,
        max_file_bytes: usize,
        mode: LocalFileMode,
    ) -> Result<(), GenerationError> {
        let files = match mode {
            LocalFileMode::Direct => self
                .snapshot
                .directory(&self.directory)
                .map(|record| record.files.clone())
                .unwrap_or_default(),
        };

        let mut section = String::from("## Local file contents\n");
        if files.is_empty() {
            section.push_str("none\n");
        } else {
            for file in files {
                let snippet = read_file_snippet(&self.snapshot.source_root, &file, max_file_bytes)?;
                self.note_primary_citation(&snippet);
                self.metadata_notes.extend(snippet.metadata_notes.clone());
                section.push_str(&snippet.render());
            }
        }
        self.sections.push(section);
        Ok(())
    }

    fn add_root_metadata_section(&mut self, max_file_bytes: usize) -> Result<(), GenerationError> {
        let files = root_metadata_files(self.snapshot);
        let manifest_names = files
            .iter()
            .filter(|path| is_manifest_file(path))
            .map(|path| path_display(path))
            .collect::<Vec<_>>();
        let workflow_names = workflow_names(&self.snapshot.source_root)?;
        let fleet_registry = fleet_services_registry(&self.snapshot.source_root);

        let mut section = String::from("## Root metadata\n");
        section.push_str(&format!(
            "manifest files: {}\n",
            list_or_none(manifest_names.iter().map(String::as_str))
        ));
        section.push_str(&format!(
            "workflow names: {}\n",
            list_or_none(workflow_names.iter().map(String::as_str))
        ));
        section.push_str(&format!(
            "fleet-services registry: {}\n",
            fleet_registry
                .as_ref()
                .map(|path| path_display(path))
                .unwrap_or_else(|| "none".to_owned())
        ));

        if files.is_empty() {
            section.push_str("metadata file contents: none\n");
        } else {
            for file in files {
                let snippet = read_file_snippet(&self.snapshot.source_root, &file, max_file_bytes)?;
                self.note_primary_citation(&snippet);
                self.metadata_notes.extend(snippet.metadata_notes.clone());
                section.push_str(&snippet.render());
            }
        }

        if let Some(registry) = fleet_registry {
            let snippet = read_file_snippet(&self.snapshot.source_root, &registry, max_file_bytes)?;
            self.note_primary_citation(&snippet);
            self.metadata_notes.extend(snippet.metadata_notes.clone());
            section.push_str(&snippet.render());
        }

        self.sections.push(section);
        Ok(())
    }

    fn add_parent_chain_section(
        &mut self,
        generated_pages: &BTreeMap<PathBuf, String>,
        existing_pages: &BTreeMap<PathBuf, String>,
    ) {
        let mut section = String::from("## Parent chain\n");
        let parents = parent_chain(&self.directory);
        if parents.is_empty() {
            section.push_str("none\n");
        } else {
            for parent in parents {
                let parent_page = generated_pages
                    .get(&parent)
                    .or_else(|| existing_pages.get(&parent));
                if let Some(html) = parent_page {
                    let distillation = distill_generated_page(&parent, html);
                    section.push_str(&format!(
                        "- {}: {}\n",
                        path_display(&parent),
                        empty_as_path_only(&distillation.what_this_is, &parent)
                    ));
                } else {
                    section.push_str(&format!("- {}\n", path_display(&parent)));
                }
            }
        }
        self.sections.push(section);
    }

    fn add_sibling_names_section(&mut self) {
        let sibling_names = sibling_dir_names(self.snapshot, &self.directory);
        let mut section = String::from("## Sibling directory names\n");
        if sibling_names.is_empty() {
            section.push_str("none\n");
        } else {
            for sibling in sibling_names {
                section.push_str(&format!("- {sibling}\n"));
            }
        }
        self.sections.push(section);
    }

    fn add_child_pages_section(
        &mut self,
        max_file_bytes: usize,
        mode: ChildPageMode,
        generated_pages: &BTreeMap<PathBuf, String>,
    ) -> Result<(), GenerationError> {
        let child_dirs = match mode {
            ChildPageMode::Direct => self
                .snapshot
                .directory(&self.directory)
                .map(|record| record.child_dirs.clone())
                .unwrap_or_default(),
            ChildPageMode::All => generated_pages
                .keys()
                .filter(|path| path.as_path() != Path::new("."))
                .cloned()
                .collect::<Vec<_>>(),
        };

        let mut section = String::from("## Child pages\n");
        if child_dirs.is_empty() {
            section.push_str("none\n");
        } else {
            for child in child_dirs {
                match generated_pages.get(&child) {
                    Some(html) => {
                        let distillation = distill_generated_page(&child, html);
                        self.note_child_citation(&distillation);
                        section.push_str(&distillation.render(max_file_bytes));
                        if html.len() > max_file_bytes {
                            self.metadata_notes.push(format!(
                                "truncated child page {} to distillation because full HTML exceeded {max_file_bytes} bytes",
                                path_display(&child)
                            ));
                        }
                    }
                    None => section.push_str(&format!(
                        "- directory: {}\n  generated_page: missing\n",
                        path_display(&child)
                    )),
                }
            }
        }
        self.sections.push(section);
        Ok(())
    }

    fn add_empty_directory_note(&mut self) {
        if let Some(record) = self.snapshot.directory(&self.directory)
            && record.files.is_empty()
            && record.child_dirs.is_empty()
        {
            self.sections.push(
                "## Empty directory\nThis directory has no analyzable local files and no child directories. Generate the empty-directory stub only.\n".to_owned(),
            );
        }
    }

    fn note_primary_citation(&mut self, snippet: &FileSnippet) {
        if self.primary_citation.is_none()
            && let Some(citation) = snippet.citation()
        {
            self.primary_citation = Some(citation);
        }
    }

    fn note_child_citation(&mut self, distillation: &PageDistillation) {
        if self.primary_citation.is_none() {
            self.primary_citation = distillation.citations.first().cloned();
        }
    }

    fn render(&self) -> String {
        self.sections.join("\n")
    }
}

#[derive(Debug, Clone, Copy)]
enum LocalFileMode {
    Direct,
}

#[derive(Debug, Clone, Copy)]
enum ChildPageMode {
    Direct,
    All,
}

struct FileSnippet {
    path: PathBuf,
    body: String,
    original_bytes: usize,
    included_bytes: usize,
    truncated: bool,
    metadata_notes: Vec<String>,
}

impl FileSnippet {
    fn line_count(&self) -> usize {
        self.body.lines().count()
    }

    fn citation(&self) -> Option<String> {
        let line_count = self.line_count();
        if line_count == 0 {
            None
        } else {
            Some(format!("{}:1-{line_count}", path_display(&self.path)))
        }
    }

    fn render(&self) -> String {
        let mut rendered = format!(
            "\n### {}\nmetadata: lines 1-{}, bytes {}/{}, truncated {}\n",
            path_display(&self.path),
            self.line_count(),
            self.included_bytes,
            self.original_bytes,
            self.truncated
        );
        if let Some(citation) = self.citation() {
            rendered.push_str(&format!("citation_range: {citation}\n"));
        }
        for (index, line) in self.body.lines().enumerate() {
            rendered.push_str(&format!("{} | {line}\n", index + 1));
        }
        rendered
    }
}

struct PageDistillation {
    directory: PathBuf,
    what_this_is: String,
    seams_contracts: String,
    where_it_can_hurt_you: String,
    citations: Vec<String>,
    full_html: String,
}

impl PageDistillation {
    fn render(&self, max_file_bytes: usize) -> String {
        let citations = if self.citations.is_empty() {
            "none".to_owned()
        } else {
            self.citations.join(", ")
        };
        let mut rendered = format!(
            "- directory: {}\n  what-this-is: {}\n  seams-contracts: {}\n  where-it-can-hurt-you: {}\n  available citations: {citations}\n",
            path_display(&self.directory),
            value_or_none(&self.what_this_is),
            value_or_none(&self.seams_contracts),
            value_or_none(&self.where_it_can_hurt_you)
        );
        if self.full_html.len() <= max_file_bytes {
            rendered.push_str("  full_html:\n");
            for line in self.full_html.lines() {
                rendered.push_str("    ");
                rendered.push_str(line);
                rendered.push('\n');
            }
        }
        rendered
    }
}

fn read_file_snippet(
    source_root: &Path,
    relative: &Path,
    max_file_bytes: usize,
) -> Result<FileSnippet, GenerationError> {
    let path = source_root.join(relative);
    let bytes = std::fs::read(&path).map_err(|source| GenerationError::Io {
        path: path.clone(),
        source,
    })?;
    let original_bytes = bytes.len();
    let (body, truncated) = utf8_safe_prefix(&bytes, max_file_bytes);
    let included_bytes = body.len();
    let mut metadata_notes = Vec::new();
    if truncated {
        metadata_notes.push(format!(
            "truncated {} to {max_file_bytes} bytes on a UTF-8 boundary",
            path_display(relative)
        ));
    }
    Ok(FileSnippet {
        path: relative.to_path_buf(),
        body,
        original_bytes,
        included_bytes,
        truncated,
        metadata_notes,
    })
}

fn distill_generated_page(directory: &Path, html: &str) -> PageDistillation {
    let document = Html::parse_document(html);
    let citations = extract_citations(&document);
    PageDistillation {
        directory: directory.to_path_buf(),
        what_this_is: extract_section_text(&document, "what-this-is"),
        seams_contracts: extract_section_text(&document, "seams-contracts"),
        where_it_can_hurt_you: extract_section_text(&document, "where-it-can-hurt-you"),
        citations,
        full_html: html.to_owned(),
    }
}

fn extract_section_text(document: &Html, section: &str) -> String {
    let selector = match Selector::parse(&format!(r#"[data-glance-section="{section}"]"#)) {
        Ok(selector) => selector,
        Err(_) => return String::new(),
    };
    let Some(element) = document.select(&selector).next() else {
        return String::new();
    };
    collapse_whitespace(&element.text().collect::<Vec<_>>().join(" "))
}

fn extract_citations(document: &Html) -> Vec<String> {
    let selector = match Selector::parse("[data-glance-cite]") {
        Ok(selector) => selector,
        Err(_) => return Vec::new(),
    };
    let mut citations = Vec::new();
    for element in document.select(&selector) {
        if let Some(raw) = element.value().attr("data-glance-cite") {
            for citation in raw
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                if !citations.iter().any(|existing| existing == citation) {
                    citations.push(citation.to_owned());
                }
            }
        }
    }
    citations
}

fn repo_one_liner(source_root: &Path) -> Option<String> {
    let readme = ["README.md", "README.markdown", "README.txt"]
        .into_iter()
        .map(|name| source_root.join(name))
        .find(|path| path.is_file())?;
    let content = std::fs::read_to_string(readme).ok()?;
    let mut heading = None;
    for line in content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if heading.is_none() && line.starts_with('#') {
            heading = Some(line.trim_start_matches('#').trim().to_owned());
            continue;
        }
        let body = line.trim_start_matches('#').trim();
        return Some(match heading {
            Some(heading) if !heading.is_empty() => format!("{heading} - {body}"),
            _ => body.to_owned(),
        });
    }
    heading
}

fn root_metadata_files(snapshot: &DirectorySnapshot) -> Vec<PathBuf> {
    snapshot
        .directory(Path::new("."))
        .map(|record| {
            record
                .files
                .iter()
                .filter(|path| is_readme_file(path) || is_manifest_file(path))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn is_readme_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_lowercase().starts_with("readme"))
        .unwrap_or(false)
}

fn is_manifest_file(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(
            "Cargo.toml"
                | "Cargo.lock"
                | "package.json"
                | "pnpm-lock.yaml"
                | "bun.lock"
                | "bun.lockb"
                | "pyproject.toml"
                | "go.mod"
                | "go.sum"
                | "deno.json"
                | "deno.jsonc"
                | "Makefile"
                | "justfile"
                | "flake.nix"
                | "glance.toml"
        )
    )
}

fn workflow_names(source_root: &Path) -> Result<Vec<String>, GenerationError> {
    let workflow_dir = source_root.join(".github/workflows");
    if !workflow_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut names = std::fs::read_dir(&workflow_dir)
        .map_err(|source| GenerationError::Io {
            path: workflow_dir.clone(),
            source,
        })?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|source| GenerationError::Io {
            path: workflow_dir.clone(),
            source,
        })?
        .into_iter()
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|file_type| file_type.is_file())
                .and_then(|_| entry.file_name().to_str().map(str::to_owned))
        })
        .collect::<Vec<_>>();
    names.sort();
    Ok(names)
}

fn fleet_services_registry(source_root: &Path) -> Option<PathBuf> {
    [
        "fleet-services.toml",
        "fleet-services.json",
        "fleet-services.yaml",
        "fleet-services.yml",
        ".fleet/services.toml",
        ".fleet/services.json",
        ".fleet/services.yaml",
        ".fleet/services.yml",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|relative| source_root.join(relative).is_file())
}

fn parent_chain(directory: &Path) -> Vec<PathBuf> {
    let mut parents = Vec::new();
    let mut current = normalize_directory(directory);
    while current != Path::new(".") {
        current = current
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        parents.push(current.clone());
    }
    parents
}

fn sibling_dir_names(snapshot: &DirectorySnapshot, directory: &Path) -> Vec<String> {
    if directory == Path::new(".") {
        return Vec::new();
    }
    let parent = directory
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut names = snapshot
        .directory(parent)
        .map(|record| {
            record
                .child_dirs
                .iter()
                .filter(|child| child.as_path() != directory)
                .filter_map(|child| child.file_name().and_then(|name| name.to_str()))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn normalize_directory(path: &Path) -> PathBuf {
    if path.as_os_str().is_empty() || path == Path::new(".") {
        PathBuf::from(".")
    } else {
        path.to_path_buf()
    }
}

fn path_display(path: &Path) -> String {
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

fn list_or_none<'a>(items: impl Iterator<Item = &'a str>) -> String {
    let values = items.collect::<Vec<_>>();
    if values.is_empty() {
        "none".to_owned()
    } else {
        values.join(", ")
    }
}

fn empty_as_path_only(value: &str, path: &Path) -> String {
    if value.trim().is_empty() {
        path_display(path)
    } else {
        value.to_owned()
    }
}

fn value_or_none(value: &str) -> String {
    if value.trim().is_empty() {
        "none".to_owned()
    } else {
        value.to_owned()
    }
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn validate_provider_output(output: &ProviderOutput) -> Result<(), GenerationError> {
    if let Some(finish_reason) = &output.finish_reason
        && is_length_finish_reason(finish_reason)
    {
        return Err(GenerationError::InvalidHtml {
            message: format!(
                "provider reported finish_reason={finish_reason}; output hit the token cap"
            ),
        });
    }
    validate_raw_html(&output.html)
}

pub(crate) fn is_retryable_output_validation(error: &GenerationError) -> bool {
    match error {
        GenerationError::InvalidHtml { message } => message.contains("data-glance-cite"),
        _ => false,
    }
}

fn validate_raw_html(html: &str) -> Result<(), GenerationError> {
    let prefix = html
        .chars()
        .take(32)
        .collect::<String>()
        .to_ascii_lowercase();
    if !(prefix.starts_with("<!doctype html") || prefix.starts_with("<html")) {
        return Err(GenerationError::InvalidHtml {
            message: "first byte was not an HTML document start".to_owned(),
        });
    }

    if !html.to_ascii_lowercase().contains("</html>") {
        return Err(GenerationError::InvalidHtml {
            message: "HTML document is missing closing </html>; provider output may be truncated"
                .to_owned(),
        });
    }

    validate_citation_attributes(html)?;
    Ok(())
}

fn validate_citation_attributes(html: &str) -> Result<(), GenerationError> {
    let document = Html::parse_document(html);
    let selector =
        Selector::parse("[data-glance-cite]").map_err(|error| GenerationError::InvalidHtml {
            message: error.to_string(),
        })?;
    for element in document.select(&selector) {
        if let Some(raw) = element.value().attr("data-glance-cite") {
            for citation in raw
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                validate_citation_grammar(citation)?;
            }
        }
    }
    Ok(())
}

fn validate_citation_grammar(citation: &str) -> Result<(), GenerationError> {
    let Some((path, range)) = citation.rsplit_once(':') else {
        return invalid_citation(citation);
    };
    if path.trim().is_empty()
        || path.chars().any(char::is_whitespace)
        || path.starts_with('/')
        || path.contains("..")
    {
        return invalid_citation(citation);
    }
    let (start, end) = match range.split_once('-') {
        Some((start, end)) => (start, end),
        None => (range, range),
    };
    let Ok(start) = start.parse::<u64>() else {
        return invalid_citation(citation);
    };
    let Ok(end) = end.parse::<u64>() else {
        return invalid_citation(citation);
    };
    if start == 0 || end < start {
        return invalid_citation(citation);
    }
    Ok(())
}

fn invalid_citation(citation: &str) -> Result<(), GenerationError> {
    Err(GenerationError::InvalidHtml {
        message: format!("invalid data-glance-cite {citation:?}: expected path:start[-end]"),
    })
}

fn is_length_finish_reason(finish_reason: &str) -> bool {
    matches!(
        finish_reason.to_ascii_lowercase().as_str(),
        "length" | "max_tokens" | "max_output_tokens"
    )
}

fn utf8_safe_prefix(bytes: &[u8], max_bytes: usize) -> (String, bool) {
    if bytes.len() <= max_bytes {
        return (String::from_utf8_lossy(bytes).into_owned(), false);
    }

    let mut end = max_bytes;
    while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
        end -= 1;
    }
    (String::from_utf8_lossy(&bytes[..end]).into_owned(), true)
}

fn estimate_tokens(text: &str) -> u64 {
    text.chars().count().div_ceil(4) as u64
}

use std::path::{Component, Path, PathBuf};
use std::process::Command;

use glance_core::{DirectorySnapshot, snapshot_tree};
use scraper::{Html, Selector};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CheckError {
    #[error("invalid citation {raw:?}: {message}")]
    InvalidCitation { raw: String, message: String },
    #[error("html selector failed: {0}")]
    Selector(String),
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Citation {
    pub path: PathBuf,
    pub start_line: usize,
    pub end_line: usize,
}

impl Citation {
    pub fn parse(raw: &str) -> Result<Self, CheckError> {
        let citations = Self::parse_many(raw)?;
        if citations.len() != 1 {
            return Err(CheckError::InvalidCitation {
                raw: raw.trim().to_owned(),
                message: "expected exactly one range".to_owned(),
            });
        }
        Ok(citations.into_iter().next().expect("one citation"))
    }

    pub fn parse_many(raw: &str) -> Result<Vec<Self>, CheckError> {
        let raw = raw.trim();
        let mut current_path: Option<PathBuf> = None;
        raw.split(',')
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .map(|segment| {
                let range = if let Some((path, range)) = segment.rsplit_once(':') {
                    let path = validate_relative_path(Path::new(path)).map_err(|message| {
                        CheckError::InvalidCitation {
                            raw: raw.to_owned(),
                            message,
                        }
                    })?;
                    current_path = Some(path);
                    range
                } else {
                    if current_path.is_none() {
                        return Err(CheckError::InvalidCitation {
                            raw: raw.to_owned(),
                            message:
                                "expected path:start[-end][,start[-end]...][,path:start[-end]...]"
                                    .to_owned(),
                        });
                    }
                    segment
                };
                let (start_line, end_line) = match range.split_once('-') {
                    Some((start, end)) => (parse_line(raw, start)?, parse_line(raw, end)?),
                    None => {
                        let line = parse_line(raw, range)?;
                        (line, line)
                    }
                };

                if start_line > end_line {
                    return Err(CheckError::InvalidCitation {
                        raw: raw.to_owned(),
                        message: "start line is after end line".to_owned(),
                    });
                }

                Ok(Self {
                    path: current_path
                        .as_ref()
                        .expect("path is set before range validation")
                        .clone(),
                    start_line,
                    end_line,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .and_then(|citations| {
                if citations.is_empty() {
                    Err(CheckError::InvalidCitation {
                        raw: raw.to_owned(),
                        message: "expected at least one range".to_owned(),
                    })
                } else {
                    Ok(citations)
                }
            })
    }

    pub fn from_html(html: &str) -> Result<Vec<Self>, CheckError> {
        let document = Html::parse_document(html);
        let selector = Selector::parse("[data-glance-cite]")
            .map_err(|error| CheckError::Selector(error.to_string()))?;
        let mut citations = Vec::new();

        for element in document.select(&selector) {
            if let Some(raw) = element.value().attr("data-glance-cite") {
                citations.extend(Self::parse_many(raw)?);
            }
        }

        Ok(citations)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitationFailure {
    pub citation: Citation,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationFailure {
    pub directory: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageContractFailure {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    pub citations_checked: usize,
    pub failures: Vec<CitationFailure>,
    pub navigation_failures: Vec<NavigationFailure>,
    pub page_contract_failures: Vec<PageContractFailure>,
}

impl CheckReport {
    pub fn is_ok(&self) -> bool {
        self.failures.is_empty()
            && self.navigation_failures.is_empty()
            && self.page_contract_failures.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct CitationChecker {
    source_root: PathBuf,
    source_sha: String,
}

impl CitationChecker {
    pub fn new(source_root: impl AsRef<Path>, source_sha: impl Into<String>) -> Self {
        Self {
            source_root: source_root.as_ref().to_path_buf(),
            source_sha: source_sha.into(),
        }
    }

    pub fn check_html(&self, html: &str) -> CheckReport {
        let citations = match Citation::from_html(html) {
            Ok(citations) => citations,
            Err(error) => {
                return CheckReport {
                    citations_checked: 0,
                    failures: vec![CitationFailure {
                        citation: Citation {
                            path: PathBuf::from("<html>"),
                            start_line: 0,
                            end_line: 0,
                        },
                        message: error.to_string(),
                    }],
                    navigation_failures: Vec::new(),
                    page_contract_failures: Vec::new(),
                };
            }
        };

        let mut failures = Vec::new();
        for citation in &citations {
            if let Err(message) = self.verify_citation(citation) {
                failures.push(CitationFailure {
                    citation: citation.clone(),
                    message,
                });
            }
        }
        let navigation_failures = self.check_navigation(html);
        let page_contract_failures = validate_page_contract(html);

        CheckReport {
            citations_checked: citations.len(),
            failures,
            navigation_failures,
            page_contract_failures,
        }
    }

    pub fn check_html_file(&self, path: impl AsRef<Path>) -> Result<CheckReport, CheckError> {
        let path = path.as_ref();
        let html = std::fs::read_to_string(path).map_err(|source| CheckError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(self.check_html(&html))
    }

    fn check_navigation(&self, html: &str) -> Vec<NavigationFailure> {
        let snapshot = match snapshot_tree(&self.source_root, self.source_sha.clone()) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return vec![NavigationFailure {
                    directory: PathBuf::from("<source>"),
                    message: format!("source snapshot failed: {error}"),
                }];
            }
        };
        validate_navigation(html, &snapshot)
    }

    fn verify_citation(&self, citation: &Citation) -> std::result::Result<(), String> {
        let content = self.read_file_at_sha(&citation.path)?;
        let line_count = content.lines().count();
        if citation.end_line > line_count {
            return Err(format!(
                "{} has {line_count} lines, citation asks for line {}",
                citation.path.display(),
                citation.end_line
            ));
        }
        Ok(())
    }

    fn read_file_at_sha(&self, path: &Path) -> std::result::Result<String, String> {
        let spec = format!(
            "{}:{}{}",
            self.source_sha,
            self.git_prefix()?,
            path_to_git_spec(path)
        );
        let output = Command::new("git")
            .args(["show", &spec])
            .current_dir(&self.source_root)
            .output()
            .map_err(|error| format!("git show failed: {error}"))?;

        if !output.status.success() {
            return Err(format!(
                "{} not found at {}",
                path.display(),
                self.source_sha
            ));
        }

        String::from_utf8(output.stdout).map_err(|error| error.to_string())
    }

    fn git_prefix(&self) -> std::result::Result<String, String> {
        let output = Command::new("git")
            .args(["rev-parse", "--show-prefix"])
            .current_dir(&self.source_root)
            .output()
            .map_err(|error| format!("git rev-parse failed: {error}"))?;

        if !output.status.success() {
            return Err(format!(
                "git rev-parse failed in {}",
                self.source_root.display()
            ));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }
}

pub fn validate_navigation(html: &str, snapshot: &DirectorySnapshot) -> Vec<NavigationFailure> {
    let document = Html::parse_document(html);
    let directory = match page_directory(&document) {
        Ok(directory) => directory,
        Err(message) => {
            return vec![NavigationFailure {
                directory: PathBuf::from("<html>"),
                message,
            }];
        }
    };

    let mut failures = Vec::new();
    let Some(record) = snapshot.directory(&directory) else {
        return vec![NavigationFailure {
            directory,
            message: "data-glance-directory does not exist in source snapshot".to_owned(),
        }];
    };

    let hrefs = page_hrefs(&document);
    if directory != Path::new(".") {
        let parent = parent_directory(&directory);
        if !hrefs
            .iter()
            .any(|href| href_targets_directory(href, &directory, &parent))
        {
            failures.push(NavigationFailure {
                directory: directory.clone(),
                message: format!(
                    "missing parent link {}",
                    directory_href(&directory, &parent)
                ),
            });
        }
    }

    for child in &record.child_dirs {
        if !hrefs
            .iter()
            .any(|href| href_targets_directory(href, &directory, child))
        {
            failures.push(NavigationFailure {
                directory: directory.clone(),
                message: format!(
                    "missing child link {} href={}",
                    path_label(child),
                    directory_href(&directory, child)
                ),
            });
        }
    }

    failures
}

pub fn validate_page_contract(html: &str) -> Vec<PageContractFailure> {
    let document = Html::parse_document(html);
    let catalog_selector = match Selector::parse("[data-glance-catalog-version]") {
        Ok(selector) => selector,
        Err(error) => {
            return vec![PageContractFailure {
                message: error.to_string(),
            }];
        }
    };
    if document.select(&catalog_selector).next().is_none() {
        return Vec::new();
    }

    let mut failures = Vec::new();
    let component_selector = match Selector::parse(".glance-main > [data-glance-component]") {
        Ok(selector) => selector,
        Err(error) => {
            return vec![PageContractFailure {
                message: error.to_string(),
            }];
        }
    };
    let mut components = document
        .select(&component_selector)
        .filter_map(|element| element.value().attr("data-glance-component"))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if components.is_empty() {
        let fallback_selector = match Selector::parse("[data-glance-component]") {
            Ok(selector) => selector,
            Err(error) => {
                return vec![PageContractFailure {
                    message: error.to_string(),
                }];
            }
        };
        components = document
            .select(&fallback_selector)
            .filter_map(|element| element.value().attr("data-glance-component"))
            .map(str::to_owned)
            .collect::<Vec<_>>();
    }

    if components.first().map(String::as_str) != Some("hero") {
        failures.push(PageContractFailure {
            message: "catalog page must start with hero".to_owned(),
        });
    }
    if !matches!(
        components.get(1).map(String::as_str),
        Some("narrative" | "flow_diagram")
    ) {
        failures.push(PageContractFailure {
            message: "catalog page must put narrative or flow_diagram immediately after hero"
                .to_owned(),
        });
    }
    let file_table_index = components
        .iter()
        .position(|component| component == "file_table");
    if components
        .iter()
        .filter(|component| component.as_str() == "file_table")
        .count()
        > 1
    {
        failures.push(PageContractFailure {
            message: "catalog page may include only one file_table".to_owned(),
        });
    }
    let story_index = components.iter().position(|component| {
        matches!(
            component.as_str(),
            "narrative" | "flow_diagram" | "callouts" | "image_figure" | "custom_html"
        )
    });
    match (story_index, file_table_index) {
        (_, None) => failures.push(PageContractFailure {
            message: "catalog page must include file_table".to_owned(),
        }),
        (Some(story), Some(table)) if story < table => {}
        _ => failures.push(PageContractFailure {
            message: "catalog page must put narrative or flow content before file_table".to_owned(),
        }),
    }

    let mut seen_disclosure = false;
    let mut seen_file_table = false;
    for component in &components {
        if seen_disclosure && component != "disclosure" {
            failures.push(PageContractFailure {
                message: "disclosure components must be last".to_owned(),
            });
            break;
        }
        if seen_file_table && component != "disclosure" {
            failures.push(PageContractFailure {
                message: "file_table must follow all story components and precede disclosures"
                    .to_owned(),
            });
            break;
        }
        if component == "disclosure" {
            seen_disclosure = true;
        }
        if component == "file_table" {
            seen_file_table = true;
        }
    }

    let body_text = document.root_element().text().collect::<Vec<_>>().join(" ");
    if contains_visible_bracket_citation(&body_text)
        || iframe_srcdoc_has_visible_bracket_citation(&document)
    {
        failures.push(PageContractFailure {
            message: "visible bracket citation noise is forbidden".to_owned(),
        });
    }

    failures
}

fn contains_visible_bracket_citation(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'[' {
            index += 1;
            continue;
        }
        let Some(end) = text[index + 1..].find(']').map(|offset| index + 1 + offset) else {
            return false;
        };
        let inside = &text[index + 1..end];
        if looks_like_bracket_citation(inside) {
            return true;
        }
        index = end + 1;
    }
    false
}

fn looks_like_bracket_citation(candidate: &str) -> bool {
    let candidate = candidate.trim();
    let Some((path, ranges)) = candidate.rsplit_once(':') else {
        return false;
    };
    if path.trim().is_empty() || ranges.trim().is_empty() || path.chars().any(char::is_whitespace) {
        return false;
    }
    ranges
        .split(',')
        .all(|range| valid_bracket_citation_range(range.trim()))
}

fn valid_bracket_citation_range(range: &str) -> bool {
    let Some((start, end)) = range.split_once('-') else {
        return range.chars().all(|character| character.is_ascii_digit()) && !range.is_empty();
    };
    !start.is_empty()
        && !end.is_empty()
        && start.chars().all(|character| character.is_ascii_digit())
        && end.chars().all(|character| character.is_ascii_digit())
}

fn iframe_srcdoc_has_visible_bracket_citation(document: &Html) -> bool {
    let Ok(selector) = Selector::parse("iframe[srcdoc]") else {
        return false;
    };
    document.select(&selector).any(|iframe| {
        let Some(srcdoc) = iframe.value().attr("srcdoc") else {
            return false;
        };
        let fragment = Html::parse_fragment(srcdoc);
        let text = fragment.root_element().text().collect::<Vec<_>>().join(" ");
        contains_visible_bracket_citation(&text)
    })
}

fn page_directory(document: &Html) -> std::result::Result<PathBuf, String> {
    let selector = Selector::parse("[data-glance-directory]").map_err(|error| error.to_string())?;
    let Some(element) = document.select(&selector).next() else {
        return Err("missing data-glance-directory".to_owned());
    };
    let raw = element
        .value()
        .attr("data-glance-directory")
        .unwrap_or_default()
        .trim();
    if raw.is_empty() {
        return Err("empty data-glance-directory".to_owned());
    }
    if raw == "." {
        return Ok(PathBuf::from("."));
    }
    validate_relative_path(Path::new(raw))
}

fn page_hrefs(document: &Html) -> Vec<String> {
    let selector = match Selector::parse("a[href]") {
        Ok(selector) => selector,
        Err(_) => return Vec::new(),
    };
    document
        .select(&selector)
        .filter_map(|element| element.value().attr("href"))
        .map(str::to_owned)
        .collect()
}

pub fn directory_href(current: &Path, target: &Path) -> String {
    let current_dir = site_directory(current);
    let target_file = site_index_file(target);
    let relative = relative_path_between(&current_dir, &target_file);
    if relative.as_os_str().is_empty() {
        "index.html".to_owned()
    } else {
        href_label(&relative)
    }
}

fn href_targets_directory(href: &str, current: &Path, target: &Path) -> bool {
    let Some(path) = href_path(href) else {
        return false;
    };
    let current_dir = site_directory(current);
    let resolved = normalize_site_path(&current_dir.join(path));
    resolved
        .as_ref()
        .is_some_and(|path| path == &site_index_file(target))
}

fn href_path(href: &str) -> Option<PathBuf> {
    let href = href
        .split('#')
        .next()
        .unwrap_or(href)
        .split('?')
        .next()
        .unwrap_or(href)
        .trim();
    if href.is_empty()
        || href.starts_with('#')
        || href.starts_with('/')
        || href.contains("://")
        || href.starts_with("mailto:")
    {
        return None;
    }
    let mut path = PathBuf::from(href);
    if href.ends_with('/') || path.extension().is_none() {
        path.push("index.html");
    }
    Some(path)
}

fn parent_directory(directory: &Path) -> PathBuf {
    directory
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn site_directory(directory: &Path) -> PathBuf {
    if directory == Path::new(".") {
        PathBuf::new()
    } else {
        directory.to_path_buf()
    }
}

fn site_index_file(directory: &Path) -> PathBuf {
    if directory == Path::new(".") {
        PathBuf::from("index.html")
    } else {
        directory.join("index.html")
    }
}

fn relative_path_between(from_dir: &Path, to_file: &Path) -> PathBuf {
    let from_parts = normal_components(from_dir);
    let to_parts = normal_components(to_file);
    let mut common = 0;
    while common < from_parts.len()
        && common < to_parts.len()
        && from_parts[common] == to_parts[common]
    {
        common += 1;
    }

    let mut relative = PathBuf::new();
    for _ in common..from_parts.len() {
        relative.push("..");
    }
    for part in &to_parts[common..] {
        relative.push(part);
    }
    relative
}

fn normalize_site_path(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if normalized.as_os_str().is_empty() {
        Some(PathBuf::from("index.html"))
    } else {
        Some(normalized)
    }
}

fn normal_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            _ => None,
        })
        .collect()
}

fn path_label(path: &Path) -> String {
    if path == Path::new(".") {
        ".".to_owned()
    } else {
        path.components()
            .filter_map(|component| match component {
                Component::Normal(part) => Some(part.to_string_lossy()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/")
    }
}

fn href_label(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            Component::ParentDir => Some("..".to_owned()),
            Component::CurDir => Some(".".to_owned()),
            Component::RootDir | Component::Prefix(_) => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn parse_line(raw: &str, value: &str) -> Result<usize, CheckError> {
    let line = value
        .parse::<usize>()
        .map_err(|_| CheckError::InvalidCitation {
            raw: raw.to_owned(),
            message: format!("line {value:?} is not a positive integer"),
        })?;
    if line == 0 {
        return Err(CheckError::InvalidCitation {
            raw: raw.to_owned(),
            message: "line numbers are 1-based".to_owned(),
        });
    }
    Ok(line)
}

fn validate_relative_path(path: &Path) -> std::result::Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("empty path".to_owned());
    }
    if path.to_string_lossy().contains([':', ',']) {
        return Err("path must not include citation separators".to_owned());
    }

    let mut cleaned = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => cleaned.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err("path must stay inside the source root".to_owned());
            }
        }
    }

    if cleaned.as_os_str().is_empty() {
        return Err("empty path".to_owned());
    }
    Ok(cleaned)
}

fn path_to_git_spec(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_parent_directory_citations() {
        assert!(Citation::parse("../secret:1").is_err());
    }

    #[test]
    fn accepts_multiple_paths_in_one_citation_attribute() {
        let citations =
            Citation::parse_many("src/lib.rs:1-2,README.md:1").expect("multi-path citation");

        assert_eq!(
            citations,
            vec![
                Citation {
                    path: PathBuf::from("src/lib.rs"),
                    start_line: 1,
                    end_line: 2,
                },
                Citation {
                    path: PathBuf::from("README.md"),
                    start_line: 1,
                    end_line: 1,
                },
            ]
        );
    }

    #[test]
    fn rejects_bare_range_before_any_path() {
        assert!(Citation::parse_many("1-2,src/lib.rs:3").is_err());
    }
}

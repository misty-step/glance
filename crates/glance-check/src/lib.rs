use std::path::{Component, Path, PathBuf};
use std::process::Command;

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
        let (path, ranges) = raw
            .rsplit_once(':')
            .ok_or_else(|| CheckError::InvalidCitation {
                raw: raw.to_owned(),
                message: "expected path:start[-end][,start[-end]...]".to_owned(),
            })?;

        let path = validate_relative_path(Path::new(path)).map_err(|message| {
            CheckError::InvalidCitation {
                raw: raw.to_owned(),
                message,
            }
        })?;

        ranges
            .split(',')
            .map(str::trim)
            .filter(|range| !range.is_empty())
            .map(|range| {
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
                    path: path.clone(),
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
pub struct CheckReport {
    pub citations_checked: usize,
    pub failures: Vec<CitationFailure>,
}

impl CheckReport {
    pub fn is_ok(&self) -> bool {
        self.failures.is_empty()
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

        CheckReport {
            citations_checked: citations.len(),
            failures,
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
    fn rejects_multiple_paths_in_one_citation_attribute() {
        assert!(Citation::parse_many("src/lib.rs:1-2,README.md:1").is_err());
    }
}

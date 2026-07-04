use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("source root does not exist: {0}")]
    MissingRoot(PathBuf),
    #[error("path is outside source root: {path}")]
    PathOutsideRoot { path: PathBuf },
    #[error("path is not a safe relative source path: {path}")]
    UnsafeRelativePath { path: PathBuf },
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("git command failed in {root}: {message}")]
    Git { root: PathBuf, message: String },
    #[error("source root has uncommitted changes and cannot be pinned to HEAD: {root}")]
    DirtyWorktree { root: PathBuf },
}

pub type Result<T> = std::result::Result<T, CoreError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkOptions {
    ignored_dir_names: BTreeSet<String>,
    ignored_relative_dirs: BTreeSet<PathBuf>,
}

impl Default for WalkOptions {
    fn default() -> Self {
        Self {
            ignored_dir_names: [".git", "target", "glance-site-out"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            ignored_relative_dirs: [PathBuf::from("tests/fixtures/live-sample")]
                .into_iter()
                .collect(),
        }
    }
}

impl WalkOptions {
    pub fn ignores_dir_name(&self, name: &str) -> bool {
        self.ignored_dir_names.contains(name)
    }

    pub fn ignores_relative_dir(&self, path: &Path) -> bool {
        self.ignored_relative_dirs.contains(&normalize_dot(path))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryRecord {
    pub relative_path: PathBuf,
    pub files: Vec<PathBuf>,
    pub child_dirs: Vec<PathBuf>,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectorySnapshot {
    pub source_root: PathBuf,
    pub source_sha: String,
    pub directories: BTreeMap<PathBuf, DirectoryRecord>,
}

impl DirectorySnapshot {
    pub fn directory(&self, relative_path: &Path) -> Option<&DirectoryRecord> {
        self.directories.get(&normalize_dot(relative_path))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegenerationPlan {
    pub directories: Vec<PathBuf>,
}

impl RegenerationPlan {
    pub fn from_snapshots(before: &DirectorySnapshot, after: &DirectorySnapshot) -> Result<Self> {
        let mut changed = BTreeSet::new();
        let keys = before
            .directories
            .keys()
            .chain(after.directories.keys())
            .cloned()
            .collect::<BTreeSet<_>>();

        for key in keys {
            let before_hash = before
                .directories
                .get(&key)
                .map(|record| &record.content_hash);
            let after_hash = after
                .directories
                .get(&key)
                .map(|record| &record.content_hash);
            if before_hash != after_hash {
                insert_ancestors(&key, &mut changed);
            }
        }

        Ok(Self {
            directories: leaf_to_root_dirs(changed),
        })
    }

    pub fn from_changed_paths<I, P>(root: impl AsRef<Path>, changed_paths: I) -> Result<Self>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let root = canonical_or_raw(root.as_ref())?;
        let mut changed_dirs = BTreeSet::new();

        for changed_path in changed_paths {
            let relative = normalize_changed_path(&root, changed_path.as_ref())?;
            let dir = directory_for_changed_path(&root, &relative);
            insert_ancestors(&dir, &mut changed_dirs);
        }

        Ok(Self {
            directories: leaf_to_root_dirs(changed_dirs),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcePin {
    pub sha: String,
}

impl SourcePin {
    pub fn resolve_git_head(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        ensure_clean_git_tree(&root)?;
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&root)
            .output()
            .map_err(|source| CoreError::Io {
                path: root.clone(),
                source,
            })?;

        if !output.status.success() {
            return Err(CoreError::Git {
                root,
                message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }

        Ok(Self {
            sha: String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        })
    }
}

pub fn snapshot_tree(
    root: impl AsRef<Path>,
    source_sha: impl Into<String>,
) -> Result<DirectorySnapshot> {
    snapshot_tree_with_options(root, source_sha, &WalkOptions::default())
}

pub fn snapshot_tree_with_options(
    root: impl AsRef<Path>,
    source_sha: impl Into<String>,
    options: &WalkOptions,
) -> Result<DirectorySnapshot> {
    let source_root = canonical_existing_root(root.as_ref())?;
    let mut directories = BTreeMap::new();
    hash_directory(&source_root, &source_root, options, &mut directories)?;

    Ok(DirectorySnapshot {
        source_root,
        source_sha: source_sha.into(),
        directories,
    })
}

pub fn leaf_to_root_dirs<I>(dirs: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = PathBuf>,
{
    let mut dirs = dirs
        .into_iter()
        .map(|path| normalize_dot(&path))
        .collect::<Vec<_>>();
    dirs.sort_by(|left, right| {
        component_count(right)
            .cmp(&component_count(left))
            .then_with(|| path_key(left).cmp(&path_key(right)))
    });
    dirs.dedup();
    dirs
}

fn hash_directory(
    directory: &Path,
    root: &Path,
    options: &WalkOptions,
    records: &mut BTreeMap<PathBuf, DirectoryRecord>,
) -> Result<String> {
    let mut entries = std::fs::read_dir(directory)
        .map_err(|source| CoreError::Io {
            path: directory.to_path_buf(),
            source,
        })?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|source| CoreError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
    entries.sort_by_key(|entry| entry.file_name());

    let mut files = Vec::new();
    let mut child_dirs = Vec::new();
    let mut child_hashes = Vec::new();

    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type().map_err(|source| CoreError::Io {
            path: path.clone(),
            source,
        })?;

        if file_type.is_dir() {
            let relative = relative_path(root, &path)?;
            if options.ignores_dir_name(os_str_to_name(&entry.file_name()).as_ref())
                || options.ignores_relative_dir(&relative)
            {
                continue;
            }
            let child_hash = hash_directory(&path, root, options, records)?;
            child_dirs.push(relative.clone());
            child_hashes.push((relative, child_hash));
        } else if file_type.is_file() || file_type.is_symlink() {
            files.push(relative_path(root, &path)?);
        }
    }

    let relative = relative_path(root, directory)?;
    let mut hasher = Sha256::new();
    hasher.update(b"glance-dir-v1\0");
    hasher.update(path_key(&relative).as_bytes());
    hasher.update(b"\0");

    for file in &files {
        let absolute = root.join(file);
        let metadata = std::fs::symlink_metadata(&absolute).map_err(|source| CoreError::Io {
            path: absolute.clone(),
            source,
        })?;
        if metadata.file_type().is_symlink() {
            let target = std::fs::read_link(&absolute).map_err(|source| CoreError::Io {
                path: absolute,
                source,
            })?;
            hasher.update(b"symlink\0");
            hasher.update(path_key(file).as_bytes());
            hasher.update(b"\0");
            hasher.update(target.to_string_lossy().as_bytes());
            hasher.update(b"\0");
        } else {
            let bytes = std::fs::read(&absolute).map_err(|source| CoreError::Io {
                path: absolute,
                source,
            })?;
            hasher.update(b"file\0");
            hasher.update(path_key(file).as_bytes());
            hasher.update(b"\0");
            hasher.update(bytes.len().to_string().as_bytes());
            hasher.update(b"\0");
            hasher.update(&bytes);
            hasher.update(b"\0");
        }
    }

    for (child, child_hash) in &child_hashes {
        hasher.update(b"child\0");
        hasher.update(path_key(child).as_bytes());
        hasher.update(b"\0");
        hasher.update(child_hash.as_bytes());
        hasher.update(b"\0");
    }

    let content_hash = hex_digest(hasher.finalize().as_slice());
    records.insert(
        relative.clone(),
        DirectoryRecord {
            relative_path: relative,
            files,
            child_dirs,
            content_hash: content_hash.clone(),
        },
    );

    Ok(content_hash)
}

fn canonical_existing_root(root: &Path) -> Result<PathBuf> {
    if !root.exists() {
        return Err(CoreError::MissingRoot(root.to_path_buf()));
    }
    root.canonicalize().map_err(|source| CoreError::Io {
        path: root.to_path_buf(),
        source,
    })
}

fn canonical_or_raw(root: &Path) -> Result<PathBuf> {
    if root.exists() {
        canonical_existing_root(root)
    } else {
        Ok(root.to_path_buf())
    }
}

fn ensure_clean_git_tree(root: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=normal"])
        .current_dir(root)
        .output()
        .map_err(|source| CoreError::Io {
            path: root.to_path_buf(),
            source,
        })?;

    if !output.status.success() {
        return Err(CoreError::Git {
            root: root.to_path_buf(),
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    if !output.stdout.is_empty() {
        return Err(CoreError::DirtyWorktree {
            root: root.to_path_buf(),
        });
    }

    Ok(())
}

fn relative_path(root: &Path, path: &Path) -> Result<PathBuf> {
    if path == root {
        return Ok(PathBuf::from("."));
    }

    let stripped = path
        .strip_prefix(root)
        .map_err(|_| CoreError::PathOutsideRoot {
            path: path.to_path_buf(),
        })?;
    validate_relative_path(stripped)
}

fn normalize_changed_path(root: &Path, changed_path: &Path) -> Result<PathBuf> {
    let relative = if changed_path.is_absolute() {
        changed_path
            .strip_prefix(root)
            .map_err(|_| CoreError::PathOutsideRoot {
                path: changed_path.to_path_buf(),
            })?
            .to_path_buf()
    } else {
        changed_path.to_path_buf()
    };
    validate_relative_path(&relative)
}

fn validate_relative_path(path: &Path) -> Result<PathBuf> {
    let mut cleaned = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => cleaned.push(part),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(CoreError::UnsafeRelativePath {
                    path: path.to_path_buf(),
                });
            }
        }
    }
    Ok(normalize_dot(&cleaned))
}

fn directory_for_changed_path(root: &Path, relative: &Path) -> PathBuf {
    if relative == Path::new(".") {
        return PathBuf::from(".");
    }

    let absolute = root.join(relative);
    if absolute.is_dir() {
        return relative.to_path_buf();
    }

    relative
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn insert_ancestors(path: &Path, dirs: &mut BTreeSet<PathBuf>) {
    let mut current = normalize_dot(path);
    loop {
        dirs.insert(current.clone());
        if current == Path::new(".") {
            break;
        }
        current = current
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
    }
}

fn normalize_dot(path: &Path) -> PathBuf {
    if path.as_os_str().is_empty() || path == Path::new(".") {
        PathBuf::from(".")
    } else {
        path.to_path_buf()
    }
}

fn component_count(path: &Path) -> usize {
    if path == Path::new(".") {
        0
    } else {
        path.components().count()
    }
}

fn path_key(path: &Path) -> String {
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

fn os_str_to_name(value: &OsStr) -> String {
    value.to_string_lossy().into_owned()
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_relative_paths() {
        assert!(validate_relative_path(Path::new("../outside")).is_err());
        assert_eq!(
            validate_relative_path(Path::new("./src/lib.rs")).expect("path"),
            PathBuf::from("src/lib.rs")
        );
    }
}

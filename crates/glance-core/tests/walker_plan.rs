use std::path::{Path, PathBuf};

use glance_core::{RegenerationPlan, WalkOptions, leaf_to_root_dirs, snapshot_tree};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mini-source")
}

#[test]
fn walker_records_direct_children_and_hashes_leaf_to_root() {
    let snapshot = snapshot_tree(fixture_root(), "fixture-sha").expect("snapshot");

    let root = snapshot.directory(Path::new(".")).expect("root record");
    assert_eq!(root.files, vec![PathBuf::from("README.md")]);
    assert_eq!(
        root.child_dirs,
        vec![PathBuf::from("docs"), PathBuf::from("src")]
    );
    assert_eq!(root.content_hash.len(), 64);

    let ordered = leaf_to_root_dirs(snapshot.directories.keys().cloned());
    assert_eq!(
        ordered,
        vec![
            PathBuf::from("src/parser"),
            PathBuf::from("docs"),
            PathBuf::from("src"),
            PathBuf::from("."),
        ]
    );
}

#[test]
fn changed_path_plan_returns_changed_dir_and_ancestors_leaf_to_root() {
    let plan =
        RegenerationPlan::from_changed_paths(fixture_root(), [PathBuf::from("src/parser/mod.rs")])
            .expect("plan");

    assert_eq!(
        plan.directories,
        vec![
            PathBuf::from("src/parser"),
            PathBuf::from("src"),
            PathBuf::from("."),
        ]
    );
}

#[test]
fn snapshot_delta_is_limited_to_changed_ancestor_path() {
    let temp = tempfile::tempdir().expect("tempdir");
    copy_dir(&fixture_root(), temp.path()).expect("copy fixture");

    let before = snapshot_tree(temp.path(), "before").expect("before snapshot");
    std::fs::write(
        temp.path().join("src/parser/mod.rs"),
        "pub fn parse_answer(_: &str) -> u32 { 7 }\n",
    )
    .expect("modify parser");
    let after = snapshot_tree(temp.path(), "after").expect("after snapshot");

    let plan = RegenerationPlan::from_snapshots(&before, &after).expect("plan");
    assert_eq!(
        plan.directories,
        vec![
            PathBuf::from("src/parser"),
            PathBuf::from("src"),
            PathBuf::from("."),
        ]
    );
}

#[test]
fn default_walk_options_exclude_git_and_target_directories() {
    let options = WalkOptions::default();
    assert!(options.ignores_dir_name(".git"));
    assert!(options.ignores_dir_name("target"));
}

#[cfg(unix)]
#[test]
fn symlink_hashes_do_not_follow_external_targets() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("tempdir");
    let external = tempfile::NamedTempFile::new().expect("external");
    std::fs::write(external.path(), "first").expect("write external");
    symlink(external.path(), temp.path().join("external-link")).expect("symlink");

    let before = snapshot_tree(temp.path(), "before").expect("before");
    std::fs::write(external.path(), "second").expect("rewrite external");
    let after = snapshot_tree(temp.path(), "after").expect("after");

    assert_eq!(
        before
            .directory(Path::new("."))
            .expect("before root")
            .content_hash,
        after
            .directory(Path::new("."))
            .expect("after root")
            .content_hash
    );
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let destination = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &destination)?;
        } else {
            std::fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}

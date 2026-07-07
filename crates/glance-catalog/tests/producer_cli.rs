//! Exercises the real compiled `glance-catalog` binary (glance-929's
//! producer seam), not just `document::CatalogDocument` in-process --
//! callers outside the Rust workspace only ever see this subprocess
//! boundary, so that's what needs a live proof. Mirrors
//! `crates/glance-mcp/tests/live_binary.rs`'s own-binary-build pattern.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("crates/glance-catalog is two levels under the workspace root")
        .to_path_buf()
}

fn binary_path() -> PathBuf {
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"));
    let path = target_dir.join("debug").join("glance-catalog");
    if path.is_file() {
        return path;
    }
    let status = Command::new(env!("CARGO"))
        .args(["build", "-p", "glance-catalog", "--bin", "glance-catalog"])
        .current_dir(workspace_root())
        .status()
        .expect("spawn cargo build -p glance-catalog --bin glance-catalog");
    assert!(
        status.success(),
        "building the glance-catalog binary failed"
    );
    assert!(
        path.is_file(),
        "expected a compiled glance-catalog binary at {}",
        path.display()
    );
    path
}

fn run_cli(json: &str, extra_args: &[&str]) -> std::process::Output {
    let mut child = Command::new(binary_path())
        .args(extra_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn glance-catalog");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(json.as_bytes())
        .expect("write spec to stdin");
    child.wait_with_output().expect("wait for glance-catalog")
}

const VALID_SPEC: &str = r#"{"catalog_version":"aesthetic-catalog-001","title":"Producer seam smoke","components":[{"type":"hero","title":"Hi","summary":[{"type":"text","text":"hello"}]},{"type":"markdown","content":"body"}]}"#;

#[test]
fn renders_a_valid_spec_to_self_contained_html_on_stdout() {
    let output = run_cli(VALID_SPEC, &[]);
    assert!(output.status.success(), "{output:#?}");
    let html = String::from_utf8(output.stdout).expect("utf8 html");
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains("<title>Producer seam smoke</title>"));
    assert!(html.contains(r#"data-glance-component="hero""#));
}

#[test]
fn rejects_malformed_json_with_exit_code_2() {
    let output = run_cli("not json", &[]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("invalid spec"));
}

#[test]
fn rejects_an_invalid_layout_with_exit_code_2() {
    let no_hero = r#"{"catalog_version":"aesthetic-catalog-001","components":[{"type":"markdown","content":"no hero first"}]}"#;
    let output = run_cli(no_hero, &[]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("invalid spec"));
}

#[test]
fn verifies_citations_against_a_real_repo_and_fails_closed_on_a_bad_one() {
    let (repo, sha) = committed_source_repo();
    let cited = r#"{"catalog_version":"aesthetic-catalog-001","components":[{"type":"hero","title":"Hi","summary":[{"type":"cite","text":"see readme","ref_id":"README.md:1-3"}]},{"type":"markdown","content":"body"}]}"#.to_string();

    let source_root = repo.path().display().to_string();
    let good = run_cli(
        &cited,
        &["--source-root", &source_root, "--source-sha", &sha],
    );
    assert!(good.status.success(), "{good:#?}");

    let bad_cited = cited.replace("README.md:1-3", "README.md:99-100");
    let bad = run_cli(
        &bad_cited,
        &["--source-root", &source_root, "--source-sha", &sha],
    );
    assert_eq!(bad.status.code(), Some(3));
    let stderr = String::from_utf8(bad.stderr).expect("utf8 stderr");
    assert!(stderr.contains("citation check failed"));
    assert!(stderr.contains("README.md"));
}

fn committed_source_repo() -> (tempfile::TempDir, String) {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(temp.path().join("README.md"), "line1\nline2\nline3\n").expect("write readme");

    run(temp.path(), ["init", "-b", "main"]);
    run(temp.path(), ["add", "."]);
    run(
        temp.path(),
        [
            "-c",
            "user.name=glance-catalog-test",
            "-c",
            "user.email=glance-catalog-test@example.invalid",
            "commit",
            "--no-verify",
            "-m",
            "fixture",
        ],
    );

    let output = Command::new("git")
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(["rev-parse", "HEAD"])
        .current_dir(temp.path())
        .output()
        .expect("git rev-parse");
    assert!(output.status.success());
    let sha = String::from_utf8(output.stdout)
        .expect("utf8")
        .trim()
        .to_owned();
    (temp, sha)
}

fn run<const N: usize>(dir: &std::path::Path, args: [&str; N]) {
    let status = Command::new("git")
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .current_dir(dir)
        .status()
        .expect("git");
    assert!(status.success());
}

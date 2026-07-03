use std::path::{Path, PathBuf};
use std::process::Command;

use glance_check::{Citation, CitationChecker};

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[test]
fn parses_citation_spans_from_generated_html() {
    let html = std::fs::read_to_string(fixture_dir().join("generated/good.html")).expect("html");
    let citations = Citation::from_html(&html).expect("citations");

    assert_eq!(citations.len(), 2);
    assert_eq!(citations[0].path, PathBuf::from("src/lib.rs"));
    assert_eq!(citations[0].start_line, 1);
    assert_eq!(citations[0].end_line, 3);
}

#[test]
fn accepts_pages_whose_citations_exist_at_pinned_sha() {
    let (repo, sha) = committed_source_repo();
    let html = std::fs::read_to_string(fixture_dir().join("generated/good.html")).expect("html");

    let report = CitationChecker::new(repo.path(), sha).check_html(&html);

    assert!(report.is_ok(), "{report:#?}");
    assert_eq!(report.citations_checked, 2);
}

#[test]
fn rejects_missing_files_and_missing_line_ranges() {
    let (repo, sha) = committed_source_repo();
    let html = std::fs::read_to_string(fixture_dir().join("generated/broken.html")).expect("html");

    let report = CitationChecker::new(repo.path(), sha).check_html(&html);

    assert!(!report.is_ok());
    assert_eq!(report.failures.len(), 2);
    assert!(
        report
            .failures
            .iter()
            .any(|failure| failure.message.contains("line"))
    );
    assert!(
        report
            .failures
            .iter()
            .any(|failure| failure.message.contains("missing.rs"))
    );
}

fn committed_source_repo() -> (tempfile::TempDir, String) {
    let temp = tempfile::tempdir().expect("tempdir");
    copy_dir(&fixture_dir().join("source"), temp.path()).expect("copy source");

    run(temp.path(), ["init", "-b", "main"]);
    run(temp.path(), ["add", "."]);
    run(
        temp.path(),
        [
            "-c",
            "user.name=glance-test",
            "-c",
            "user.email=glance-test@example.invalid",
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

fn run<const N: usize>(dir: &Path, args: [&str; N]) {
    let status = Command::new("git")
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .current_dir(dir)
        .status()
        .expect("git");
    assert!(status.success());
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

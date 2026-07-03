use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use glance_core::snapshot_tree;
use glance_gen::{PageKind, assemble_prompt_context};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../glance-core/tests/fixtures/mini-source")
}

#[test]
fn leaf_context_contains_only_leaf_inputs() {
    let snapshot = snapshot_tree(fixture_root(), "fixture-sha").expect("snapshot");
    let context = assemble_prompt_context(
        &snapshot,
        Path::new("docs"),
        PageKind::Leaf,
        4096,
        &BTreeMap::new(),
        &BTreeMap::new(),
    )
    .expect("context");

    assert_eq!(context.prompt_version, "glance-005-leaf-v2");
    assert!(context.prompt.contains("## Repository"));
    assert!(context.prompt.contains("- source_sha: fixture-sha"));
    assert!(context.prompt.contains("- directory: docs"));
    assert!(context.prompt.contains("- kind: leaf"));
    assert!(context.prompt.contains("## Local file contents"));
    assert!(context.prompt.contains("### docs/guide.md"));
    assert!(context.prompt.contains("1 | # Guide"));
    assert!(context.prompt.contains("## Parent chain"));
    assert!(context.prompt.contains("- ."));
    assert!(context.prompt.contains("## Sibling directory names"));
    assert!(context.prompt.contains("- src"));
    assert!(!context.prompt.contains("## Child pages"));
    assert!(!context.prompt.contains("src/lib.rs"));
}

#[test]
fn interior_context_distills_generated_children_and_parent_chain() {
    let snapshot = snapshot_tree(fixture_root(), "fixture-sha").expect("snapshot");
    let mut generated_pages = BTreeMap::new();
    generated_pages.insert(
        PathBuf::from("src/parser"),
        child_page_html(
            "Parser translates numeric text into the answer shape.",
            "The parser exposes one public function.",
            "Invalid numeric text panics through expect.",
            "src/parser/mod.rs:1-2",
        ),
    );

    let context = assemble_prompt_context(
        &snapshot,
        Path::new("src"),
        PageKind::Interior,
        4096,
        &generated_pages,
        &BTreeMap::new(),
    )
    .expect("context");

    assert_eq!(context.prompt_version, "glance-005-interior-v2");
    assert!(context.prompt.contains("### src/lib.rs"));
    assert!(context.prompt.contains("## Child pages"));
    assert!(context.prompt.contains("- directory: src/parser"));
    assert!(
        context
            .prompt
            .contains("what-this-is: Parser translates numeric text into the answer shape.")
    );
    assert!(
        context
            .prompt
            .contains("seams-contracts: The parser exposes one public function.")
    );
    assert!(
        context
            .prompt
            .contains("where-it-can-hurt-you: Invalid numeric text panics through expect.")
    );
    assert!(
        context
            .prompt
            .contains("available citations: src/parser/mod.rs:1-2")
    );
    assert!(context.prompt.contains("## Parent chain"));
    assert!(context.prompt.contains("- ."));
    assert!(!context.prompt.contains("## Sibling directory names"));
}

#[test]
fn root_context_uses_repo_metadata_and_all_child_pages() {
    let snapshot = snapshot_tree(fixture_root(), "fixture-sha").expect("snapshot");
    let mut generated_pages = BTreeMap::new();
    generated_pages.insert(
        PathBuf::from("docs"),
        child_page_html(
            "Docs explain the guide.",
            "Docs have no outward code seam.",
            "Nothing sharp found.",
            "docs/guide.md:1-3",
        ),
    );
    generated_pages.insert(
        PathBuf::from("src"),
        child_page_html(
            "Source wires the parser.",
            "Source exports parser behavior.",
            "Parser panics can surface through the public function.",
            "src/lib.rs:1-4",
        ),
    );
    generated_pages.insert(
        PathBuf::from("src/parser"),
        child_page_html(
            "Parser parses numeric input.",
            "Parser has one public function.",
            "Bad input panics.",
            "src/parser/mod.rs:1-2",
        ),
    );

    let context = assemble_prompt_context(
        &snapshot,
        Path::new("."),
        PageKind::Root,
        4096,
        &generated_pages,
        &BTreeMap::new(),
    )
    .expect("context");

    assert_eq!(context.prompt_version, "glance-005-root-v2");
    assert!(context.prompt.contains("## Root metadata"));
    assert!(context.prompt.contains("### README.md"));
    assert!(context.prompt.contains("manifest files: none"));
    assert!(context.prompt.contains("workflow names: none"));
    assert!(context.prompt.contains("## Child pages"));
    assert!(context.prompt.contains("- directory: docs"));
    assert!(context.prompt.contains("- directory: src"));
    assert!(context.prompt.contains("- directory: src/parser"));
    assert!(context.prompt.contains("## Root-only obligations"));
    assert!(!context.prompt.contains("### src/lib.rs"));
}

#[test]
fn context_truncates_on_utf8_boundary_and_records_metadata() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(temp.path().join("README.md"), "fixture repo").expect("readme");
    std::fs::create_dir(temp.path().join("leaf")).expect("leaf dir");
    std::fs::write(temp.path().join("leaf/data.txt"), "abc😄def").expect("fixture");
    let snapshot = snapshot_tree(temp.path(), "fixture-sha").expect("snapshot");

    let context = assemble_prompt_context(
        &snapshot,
        Path::new("leaf"),
        PageKind::Leaf,
        6,
        &BTreeMap::new(),
        &BTreeMap::new(),
    )
    .expect("context");

    assert!(context.prompt.contains("abc"));
    assert!(!context.prompt.contains("def"));
    assert!(
        context
            .metadata_notes
            .iter()
            .any(|note| note.contains("truncated leaf/data.txt"))
    );
}

fn child_page_html(what_this_is: &str, seams: &str, hurt_you: &str, citation: &str) -> String {
    format!(
        r#"<!doctype html><html><body class="glance-page">
<section class="glance-section" data-glance-section="what-this-is"><p class="glance-cited" data-glance-cite="{citation}">{what_this_is}</p></section>
<section class="glance-section" data-glance-section="seams-contracts"><p class="glance-cited" data-glance-cite="{citation}">{seams}</p></section>
<section class="glance-section" data-glance-section="where-it-can-hurt-you"><p class="glance-cited" data-glance-cite="{citation}">{hurt_you}</p></section>
</body></html>"#
    )
}

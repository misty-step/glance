//! Exercises the real compiled `glance` binary through the MCP dispatch path,
//! not just a mocked subprocess. `cargo test --workspace` builds every
//! member's binaries as a side effect (including `glance`) before any test
//! runs, so the debug binary is on disk by the time this test starts.

use std::path::PathBuf;

use serde_json::json;

fn glance_debug_binary() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/glance-mcp is two levels under the workspace root");
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root.join("target"));
    target_dir.join("debug").join("glance")
}

fn with_glance_bin<T>(run: impl FnOnce() -> T) -> T {
    let binary = glance_debug_binary();
    assert!(
        binary.is_file(),
        "expected a compiled glance binary at {}; run `cargo build -p glance` first",
        binary.display()
    );
    // SAFETY: test-only; sets an env var this same test reads back via
    // glance_mcp::call_tool. Tests in this file do not run this closure
    // concurrently with a differing GLANCE_BIN.
    unsafe {
        std::env::set_var("GLANCE_BIN", &binary);
    }
    let result = run();
    unsafe {
        std::env::remove_var("GLANCE_BIN");
    }
    result
}

#[test]
fn plan_tool_runs_the_real_binary_against_the_mini_source_fixture() {
    with_glance_bin(|| {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../glance-core/tests/fixtures/mini-source");
        let response = glance_mcp::call_tool(
            "plan",
            &json!({
                "root": root.display().to_string(),
                "changed": ["src/parser/mod.rs"],
            }),
        )
        .expect("plan tool call");

        let text = response["content"][0]["text"].as_str().expect("text");
        let payload: serde_json::Value = serde_json::from_str(text).expect("json payload");
        assert_eq!(payload["exit_code"], 0);
        let stdout = payload["stdout"].as_str().expect("stdout");
        assert!(stdout.contains("src/parser"));
        assert!(stdout.contains("src"));
    });
}

#[test]
fn check_tool_surfaces_a_nonzero_exit_code_without_panicking() {
    with_glance_bin(|| {
        let response = glance_mcp::call_tool(
            "check",
            &json!({
                "source_root": "/nonexistent/source/root",
                "source_sha": "deadbeef",
                "html": ["/nonexistent/page.html"],
            }),
        )
        .expect("check tool call returns a structured result even on CLI failure");

        let text = response["content"][0]["text"].as_str().expect("text");
        let payload: serde_json::Value = serde_json::from_str(text).expect("json payload");
        assert_ne!(payload["exit_code"], 0);
    });
}

//! Exercises the real compiled `glance` binary through the MCP dispatch path,
//! not just a mocked subprocess.
//!
//! `cargo test --workspace` does not guarantee `glance`'s binary is already
//! linked to disk before this crate's test binary starts running -- cargo
//! can stream-start an already-built test binary while sibling crates are
//! still compiling, and a fresh CI checkout has no head start from an
//! earlier manual `cargo build`. So this test builds the binary itself if
//! it isn't already there, rather than assuming build order.

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use serde_json::json;

/// Serializes env-var mutation across every test in this binary. Mirrors
/// `glance::canary::test_support::env_lock` -- process env is shared across
/// threads, and `cargo test` runs tests in this file's binary in parallel by
/// default, so any future test that also touches process env needs the same
/// guard to avoid racing this one.
fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// `run_glance_with_bin` spawns the real `glance` binary via
/// `Command::new(bin).args(args).output()`, which inherits this test
/// process's full env. `glance::main` unconditionally calls
/// `canary::check_in()`, and `check` deliberately points at a nonexistent
/// source root so `glance` exits non-zero and calls `canary::report_error`.
/// If this process happens to carry real Canary credentials (e.g. a dev
/// shell exported against production `canary-obs.fly.dev`), the spawned
/// child would fire real check-ins and a real error against production
/// under service `glance-next`. Clearing these three vars here -- not in
/// `glance-mcp`'s library code, which must not change production behavior
/// -- keeps this test hermetic regardless of the caller's shell.
fn clear_canary_env() {
    // SAFETY: caller holds `env_lock()` for the duration of any env
    // mutation in this test binary, so no other thread observes a torn
    // read/write of these vars.
    unsafe {
        std::env::remove_var("CANARY_ENDPOINT");
        std::env::remove_var("CANARY_API_KEY");
        std::env::remove_var("CANARY_INGEST_KEY");
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/glance-mcp is two levels under the workspace root")
        .to_path_buf()
}

fn glance_debug_binary() -> PathBuf {
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"));
    target_dir.join("debug").join("glance")
}

fn ensure_glance_binary() -> PathBuf {
    let binary = glance_debug_binary();
    if binary.is_file() {
        return binary;
    }
    let status = Command::new(env!("CARGO"))
        .args(["build", "-p", "glance"])
        .current_dir(workspace_root())
        .status()
        .expect("spawn cargo build -p glance");
    assert!(status.success(), "cargo build -p glance failed");
    assert!(
        binary.is_file(),
        "expected a compiled glance binary at {} after building it",
        binary.display()
    );
    binary
}

#[test]
fn mcp_tools_run_and_check_dispatch_to_the_real_binary() {
    let _env_guard = env_lock().lock().expect("env lock");
    // Must run before the child is spawned below: see `clear_canary_env`.
    clear_canary_env();

    let binary = ensure_glance_binary();
    // SAFETY: guarded by `env_lock` above, so no concurrent mutation from a
    // sibling test in this binary.
    unsafe {
        std::env::set_var("GLANCE_BIN", &binary);
    }

    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../glance-core/tests/fixtures/mini-source");
    let plan_response = glance_mcp::call_tool(
        "plan",
        &json!({
            "root": root.display().to_string(),
            "changed": ["src/parser/mod.rs"],
        }),
    )
    .expect("plan tool call");
    let plan_text = plan_response["content"][0]["text"].as_str().expect("text");
    let plan_payload: serde_json::Value = serde_json::from_str(plan_text).expect("json payload");
    assert_eq!(plan_payload["exit_code"], 0);
    let stdout = plan_payload["stdout"].as_str().expect("stdout");
    assert!(stdout.contains("src/parser"));
    assert!(stdout.contains("src"));

    let check_response = glance_mcp::call_tool(
        "check",
        &json!({
            "source_root": "/nonexistent/source/root",
            "source_sha": "deadbeef",
            "html": ["/nonexistent/page.html"],
        }),
    )
    .expect("check tool call returns a structured result even on CLI failure");
    let check_text = check_response["content"][0]["text"].as_str().expect("text");
    let check_payload: serde_json::Value = serde_json::from_str(check_text).expect("json payload");
    assert_ne!(check_payload["exit_code"], 0);

    unsafe {
        std::env::remove_var("GLANCE_BIN");
    }
}

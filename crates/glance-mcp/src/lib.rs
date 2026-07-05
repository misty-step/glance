#![forbid(unsafe_code)]

//! MCP server for glance. Wraps the compiled `glance` CLI's four core verbs
//! (run, plan, check, publish) as JSON-RPC tools instead of duplicating their
//! logic: the CLI is already the tested, gated implementation, and MCP is a
//! protocol translation over it, not a second copy of the behavior.

use std::process::Command;

use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: &'static str,
}

pub const TOOLS: &[ToolDef] = &[
    ToolDef {
        name: "run",
        description: "Generate a glance site for a source repo: walk the tree, generate a page per directory, render images, write the static site. Mirrors `glance run`.",
        input_schema: r#"{"type":"object","properties":{"config":{"type":"string","description":"path to glance.toml (defaults to glance.toml in the working directory)"},"root":{"type":"string","description":"source repo root"},"site_root":{"type":"string","description":"output directory for the generated site"}}}"#,
    },
    ToolDef {
        name: "plan",
        description: "Compute the regeneration plan (which directories need a new page) for a set of changed source paths. Mirrors `glance plan`.",
        input_schema: r#"{"type":"object","properties":{"config":{"type":"string"},"root":{"type":"string","description":"source repo root"},"changed":{"type":"array","items":{"type":"string"},"description":"changed file paths relative to root"}}}"#,
    },
    ToolDef {
        name: "check",
        description: "Validate citations and parent/child/sibling navigation on one or more generated HTML pages against the source repo. Mirrors `glance check`.",
        input_schema: r#"{"type":"object","required":["html"],"properties":{"config":{"type":"string"},"source_root":{"type":"string"},"source_sha":{"type":"string"},"html":{"type":"array","minItems":1,"items":{"type":"string"},"description":"generated HTML file paths to check"}}}"#,
    },
    ToolDef {
        name: "publish",
        description: "Publish a generated site directory into the <source-name>-glance sister repository, in master or branch mode. Mirrors `glance publish`.",
        input_schema: r#"{"type":"object","required":["site_dir","source_owner","source_name","source_sha","mode"],"properties":{"config":{"type":"string"},"site_dir":{"type":"string"},"source_owner":{"type":"string"},"source_name":{"type":"string"},"source_sha":{"type":"string"},"mode":{"type":"string","enum":["branch","master"]},"sister_worktree":{"type":"string"},"sister_remote":{"type":"string"},"branch":{"type":"string"},"source_pr_title":{"type":"string"},"run_id":{"type":"string"}}}"#,
    },
];

pub fn tools() -> &'static [ToolDef] {
    TOOLS
}

pub fn tool_defs_json() -> Value {
    Value::Array(
        TOOLS
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "inputSchema": serde_json::from_str::<Value>(tool.input_schema)
                        .expect("tool schema is valid json"),
                })
            })
            .collect(),
    )
}

pub fn handle_json_rpc(request: &Value) -> Option<Value> {
    let id = request.get("id").cloned();
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");

    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": request["params"]["protocolVersion"]
                .as_str()
                .unwrap_or("2024-11-05"),
            "serverInfo": {"name": "glance", "version": env!("CARGO_PKG_VERSION")},
            "capabilities": {"tools": {"listChanged": false}},
        })),
        "tools/list" => Ok(json!({ "tools": tool_defs_json() })),
        "tools/call" => {
            let params = &request["params"];
            let name = params["name"].as_str().unwrap_or("");
            let args = &params["arguments"];
            call_tool(name, args)
        }
        "ping" => Ok(json!({})),
        other => Err(format!("method not found: {other}")),
    };

    id.map(|id| match result {
        Ok(value) => json!({"jsonrpc": "2.0", "id": id, "result": value}),
        Err(message) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32603, "message": message},
        }),
    })
}

/// The compiled `glance` binary to shell out to. Overridable via `GLANCE_BIN`
/// (e.g. a debug build under `target/debug/glance`); defaults to `glance` on
/// `PATH`.
fn glance_bin() -> String {
    std::env::var("GLANCE_BIN").unwrap_or_else(|_| "glance".to_owned())
}

pub fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
    let mut command_args: Vec<String> = Vec::new();
    if let Some(config) = optional_str(args, "config") {
        command_args.push("--config".to_owned());
        command_args.push(config.to_owned());
    }

    match name {
        "run" => {
            command_args.push("run".to_owned());
            push_opt(&mut command_args, "--root", optional_str(args, "root"));
            push_opt(
                &mut command_args,
                "--site-root",
                optional_str(args, "site_root"),
            );
        }
        "plan" => {
            command_args.push("plan".to_owned());
            push_opt(&mut command_args, "--root", optional_str(args, "root"));
            for changed in string_array(args, "changed")? {
                command_args.push("--changed".to_owned());
                command_args.push(changed);
            }
        }
        "check" => {
            command_args.push("check".to_owned());
            push_opt(
                &mut command_args,
                "--source-root",
                optional_str(args, "source_root"),
            );
            push_opt(
                &mut command_args,
                "--source-sha",
                optional_str(args, "source_sha"),
            );
            let html = string_array(args, "html")?;
            if html.is_empty() {
                return Err("check requires at least one html path".to_owned());
            }
            command_args.extend(html);
        }
        "publish" => {
            command_args.push("publish".to_owned());
            command_args.push("--site-dir".to_owned());
            command_args.push(required_str(args, "site_dir")?);
            command_args.push("--source-owner".to_owned());
            command_args.push(required_str(args, "source_owner")?);
            command_args.push("--source-name".to_owned());
            command_args.push(required_str(args, "source_name")?);
            command_args.push("--source-sha".to_owned());
            command_args.push(required_str(args, "source_sha")?);
            command_args.push("--mode".to_owned());
            command_args.push(required_str(args, "mode")?);
            push_opt(
                &mut command_args,
                "--sister-worktree",
                optional_str(args, "sister_worktree"),
            );
            push_opt(
                &mut command_args,
                "--sister-remote",
                optional_str(args, "sister_remote"),
            );
            push_opt(&mut command_args, "--branch", optional_str(args, "branch"));
            push_opt(
                &mut command_args,
                "--source-pr-title",
                optional_str(args, "source_pr_title"),
            );
            push_opt(&mut command_args, "--run-id", optional_str(args, "run_id"));
        }
        other => return Err(format!("unknown tool: {other}")),
    }

    run_glance(&command_args)
}

fn run_glance(args: &[String]) -> Result<Value, String> {
    run_glance_with_bin(&glance_bin(), args)
}

fn run_glance_with_bin(bin: &str, args: &[String]) -> Result<Value, String> {
    let output = Command::new(bin).args(args).output().map_err(|error| {
        format!(
            "failed to spawn {bin} {}: {error} (set GLANCE_BIN to the compiled binary path)",
            args.join(" ")
        )
    })?;

    let payload = json!({
        "command": format!("{bin} {}", args.join(" ")),
        "exit_code": output.status.code(),
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
    });
    let text = serde_json::to_string_pretty(&payload).map_err(|error| error.to_string())?;
    Ok(json!({"content": [{"type": "text", "text": text}]}))
}

fn push_opt(command_args: &mut Vec<String>, flag: &str, value: Option<&str>) {
    if let Some(value) = value {
        command_args.push(flag.to_owned());
        command_args.push(value.to_owned());
    }
}

fn optional_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn required_str(args: &Value, key: &str) -> Result<String, String> {
    optional_str(args, key)
        .map(str::to_owned)
        .ok_or_else(|| format!("{key} is required"))
}

fn string_array(args: &Value, key: &str) -> Result<Vec<String>, String> {
    match args.get(key) {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("{key} must be an array of strings"))
            })
            .collect(),
        Some(_) => Err(format!("{key} must be an array of strings")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_defs_include_all_four_core_verbs() {
        let names: Vec<&str> = TOOLS.iter().map(|tool| tool.name).collect();
        assert_eq!(names, vec!["run", "plan", "check", "publish"]);
    }

    #[test]
    fn call_tool_rejects_unknown_tool_name() {
        let error = call_tool("nonexistent", &json!({})).unwrap_err();
        assert!(error.contains("unknown tool"));
    }

    #[test]
    fn call_tool_check_requires_at_least_one_html_path() {
        let error = call_tool("check", &json!({"html": []})).unwrap_err();
        assert!(error.contains("at least one html path"));
    }

    #[test]
    fn call_tool_publish_requires_all_required_fields() {
        let error = call_tool("publish", &json!({"site_dir": "/tmp/site"})).unwrap_err();
        assert!(error.contains("source_owner"));
    }

    #[test]
    fn run_glance_reports_a_clear_error_when_the_binary_is_missing() {
        let error = run_glance_with_bin("glance-mcp-binary-that-does-not-exist", &[]).unwrap_err();
        assert!(error.contains("failed to spawn"));
        assert!(error.contains("GLANCE_BIN"));
    }

    #[test]
    fn handle_json_rpc_lists_tools() {
        let request = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});
        let response = handle_json_rpc(&request).expect("response");
        let tools = response["result"]["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 4);
    }

    #[test]
    fn handle_json_rpc_reports_unknown_method() {
        let request = json!({"jsonrpc": "2.0", "id": 1, "method": "not/a/method"});
        let response = handle_json_rpc(&request).expect("response");
        assert!(
            response["error"]["message"]
                .as_str()
                .unwrap()
                .contains("method not found")
        );
    }

    #[test]
    fn handle_json_rpc_returns_none_for_notifications_without_id() {
        let request = json!({"jsonrpc": "2.0", "method": "ping"});
        assert!(handle_json_rpc(&request).is_none());
    }
}

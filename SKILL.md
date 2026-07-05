---
name: glance
description: |
  Use when an agent needs to generate, check, or publish a citation-gated
  understanding site for a source repository, or orchestrate a glance run as
  a tool call instead of shelling out to the CLI by hand. Glance walks a
  source tree, generates a page per directory with a real model or the
  deterministic mock provider, checks every citation and navigation link
  against the source, and publishes the result into a `<name>-glance`
  sister repository.
argument-hint: "[run|plan|check|publish]"
---

# Glance

Glance is a Rust workspace that generates hash-gated, citation-checked HTML
understanding sites. It exposes one core (`glance-core` walking/hashing,
`glance-gen` generation and rendering, `glance-check` citation and navigation
validation, `glance-publish` sister-repo publishing) through a CLI, this
skill, and an MCP server (`glance-mcp`) that wraps the same four verbs as
JSON-RPC tools instead of duplicating their logic.

Read `README.md` for the generation-provider contract (mock vs real,
budget enforcement, tier routing) before changing `glance.toml` defaults or
provider wiring.

## Operating Contract

- `plan` before `run`: know which directories actually need regeneration
  from a changed-paths list before spending a generation budget on the
  whole tree.
- `run` writes a site directory; it never pushes anywhere. `publish` is the
  only verb that touches a remote.
- `check` is the citation and navigation gate — always run it against
  freshly generated HTML before publishing; a page with a broken cited line
  range or a dangling nav link must never ship.
- `publish` in `master` mode overwrites the sister repo's `master` branch;
  `branch` mode opens a mirrored branch and prints a `pr_url` instead. Use
  `branch` mode unless the operator has explicitly authorized a direct
  master publish.
- `provider_mode = "real"` in `glance.toml` spends real tokens against
  OpenRouter/Gemini. Never flip a tracked `glance.toml` to `real` for a CI
  gate; use a separate untracked config for a one-off real run (see
  `docs/self-run-2026-07-03.md` for the pattern).

## Expected MCP Tools

`glance-mcp` (workspace member `crates/glance-mcp`) exposes exactly the
CLI's four core verbs, no more:

- `run`: generate a site from a source root into a site-root directory.
  Args: `config?`, `root?`, `site_root?`.
- `plan`: compute the regeneration plan for a set of changed source paths.
  Args: `config?`, `root?`, `changed?` (array of paths).
- `check`: validate citations and navigation on one or more generated HTML
  pages. Args: `config?`, `source_root?`, `source_sha?`, `html` (required,
  at least one path).
- `publish`: publish a site directory into the sister repo. Args:
  `config?`, `site_dir`, `source_owner`, `source_name`, `source_sha`,
  `mode` (`branch`|`master`), `sister_worktree?`, `sister_remote?`,
  `branch?`, `source_pr_title?`, `run_id?` (all required fields, well,
  required).

Every tool call shells out to the compiled `glance` binary (never
duplicates its logic) and returns a structured result: `{command, exit_code,
stdout, stderr}` inside the standard MCP `content[0].text` JSON-RPC field.
A non-zero `exit_code` is not a transport error — read `stderr` for the
actual CLI failure message.

## Running the MCP Server

`glance-mcp` looks for the compiled `glance` binary on `PATH` by default;
override with `GLANCE_BIN` to point at a specific build (a debug build
under `target/debug/glance`, or an installed release binary):

```sh
cargo build --release -p glance -p glance-mcp
GLANCE_BIN=$(pwd)/target/release/glance ./target/release/glance-mcp
```

Smoke-test the JSON-RPC handshake and tool list directly over stdio:

```sh
printf '%s\n%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
  | GLANCE_BIN=$(pwd)/target/debug/glance ./target/debug/glance-mcp
```

## CLI

```sh
glance plan --root /path/to/source --changed src/lib.rs
glance run --root /path/to/source --site-root site
glance check --source-root /path/to/source --source-sha <sha> site/index.html
glance publish \
  --site-dir site \
  --source-owner misty-step \
  --source-name demo \
  --source-sha <sha> \
  --mode branch \
  --branch glance/source-pr-12 \
  --source-pr-title "Source PR title"
glance serve-local --site-root site --port 4173
```

`glance.toml` supplies defaults for the CLI (and, through `--config`, for
MCP tool calls); command flags/tool arguments override the file.

## Local Gate

```sh
scripts/check.sh
```

Runs formatting, Clippy with warnings denied, the full workspace test suite
(including `glance-mcp`'s live-binary integration tests, which build and
shell out to the real `glance` binary), and the shell smoke test.

## Red Lines

- Never commit real provider API keys or spend logs into this repository;
  `glance.toml`'s tracked defaults stay `provider_mode = "mock"`.
- Never let `check` pass on a page with a citation or navigation defect to
  make a gate green — the citation gate is the entire point of the product.
- Never publish in `master` mode without an explicit operator go-ahead;
  default to `branch` mode.

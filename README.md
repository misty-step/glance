# glance

Glance is a Rust workspace for generating hash-gated, citation-checked HTML
understanding sites for source repositories.

This founding slice intentionally ships only the deterministic core:

- `glance-core`: source tree walking, per-directory content hashes, regeneration
  plans, and source SHA pinning.
- `glance-check`: deterministic citation checking for generated HTML.
- `glance-publish`: sister-repo publishing for generated HTML and metadata.
- `glance-gen`: tier-routed generation providers, fallback policy, budget
  enforcement, spend reporting, and the mock provider used by deterministic
  local smoke runs.
- `glance`: CLI entrypoint for `run`, `plan`, `check`, `publish`, and
  `serve-local`.

Page templates, webhook triggers, and deployed site serving are backlog work.
The old Go tool at `phrazzld/glance` is reference material only.

## Generation providers

`glance.toml` configures leaf, interior, and root generation tiers with a model
slug and max token cap. Defaults are:

- leaf: `deepseek/deepseek-v4-flash`, 6000 output tokens
- interior: `anthropic/claude-sonnet-5`, 10000 output tokens
- root: `openai/gpt-5.5`, 16000 output tokens

Normal repo gates use `provider_mode = "mock"` so CI never depends on live
secrets. Set `provider_mode = "real"` to use env-only provider credentials:
`OPENROUTER_API_KEY` for OpenRouter chat completions and `GEMINI_API_KEY` for
Gemini native `generateContent`. The composite fallback client owns all retry,
exponential backoff, and jitter; inner provider clients make one HTTP attempt.

Budgets are hard caps in cost micros. A run fails before a provider call if the
estimated page would exceed `per_run_micros` or `per_day_micros`. Each run emits
a spend report with input tokens, output tokens, and estimated cost per page.

## Gate

Run the same command locally and in GitHub Actions:

```sh
scripts/check.sh
```

The gate runs formatting, Clippy with warnings denied, and the full workspace
test suite.

## CLI sketch

```sh
glance plan --root /path/to/source --changed src/lib.rs
glance check --source-root /path/to/source --source-sha <sha> site/index.html
glance run --config glance.toml
glance serve-local --site-root site --port 4173
glance publish \
  --site-dir site \
  --source-owner misty-step \
  --source-name demo \
  --source-sha <sha> \
  --mode branch \
  --branch glance/source-pr-12 \
  --source-pr-title "Source PR title"
```

`glance.toml` supplies defaults for the CLI. Command flags override the file.
`glance publish` writes only generated HTML plus metadata into the
`<source-name>-glance` sister repository. In `master` mode it pushes the sister
`master` branch. In `branch` mode it pushes a mirrored branch and prints
`pr_url=<url>` for the source-side comment. Local tests use `file://` bare
remotes; live GitHub creation is isolated behind the `gh` CLI path and opt-in
smoke coverage.

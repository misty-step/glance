# glance

Glance is a Rust workspace for generating hash-gated, citation-checked HTML
understanding sites for source repositories.

This founding slice intentionally ships only the deterministic core:

- `glance-core`: source tree walking, per-directory content hashes, regeneration
  plans, and source SHA pinning.
- `glance-check`: deterministic citation checking for generated HTML.
- `glance-gen`: model generation scaffolding, tier routing, and a mock provider.
- `glance-publish`: sister-repo publishing for generated HTML and metadata.
- `glance`: CLI entrypoint for `run`, `plan`, `check`, and `serve-local`.

Generation providers, page templates, webhook triggers, and deployed site
serving are backlog work. The old Go tool at `phrazzld/glance` is reference
material only.

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

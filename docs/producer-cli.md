# glance-catalog producer CLI (glance-929)

`glance-922`'s criterion 2 asked for a seam any producer can call: give it a
spec, get a validated, self-contained HTML artifact. `crates/glance-catalog`
is a Rust library; a non-Rust producer (Python, a shell pipeline, another
language's runtime) reaches it through the compiled `glance-catalog` binary
over stdin/stdout, not by linking the crate.

## Contract

**Schema.** The spec is JSON matching
`crates/glance-catalog/catalog/catalog.schema.json`'s top-level object:

```json
{
  "catalog_version": "aesthetic-catalog-001",
  "title": "optional page title",
  "layout_profile": "report",
  "components": [ /* Component values, per the same schema */ ]
}
```

`catalog_version` must equal `glance_catalog::CATALOG_VERSION` exactly.
`title` and `layout_profile` (`"stream"` or `"report"`, default `"report"`)
are optional. `components` follows `catalog.schema.json`'s `component`
union (13 kinds; see the schema's own `$defs` for every field).

**Invocation.**

```sh
glance-catalog [--input FILE] [--output FILE] \
               [--source-root DIR --source-sha SHA] \
               < spec.json > page.html
```

- `--input`/`--output` default to stdin/stdout.
- `--source-root`/`--source-sha` are optional and must be given together.
  Supply them when the spec's `Cite` nodes use the `path:lines` ref_id
  scheme (glance-gen's own convention) and you want them verified against a
  real pinned commit. Omit both for specs whose `Cite` ref_ids use a
  different scheme (e.g. an opaque evidence-pack id) -- `glance_catalog`'s
  `Cite` type is deliberately scheme-agnostic (see
  `crates/glance-catalog/src/inline.rs`), so this verification is opt-in,
  never forced.

**Exit codes.**

| Code | Meaning | Where the message goes |
| --- | --- | --- |
| `0` | Success. HTML written to `--output`/stdout. | — |
| `1` | Usage or I/O error (bad flags, unreadable input, unwritable output). | stderr, prefixed `error:` |
| `2` | Invalid spec: malformed JSON, wrong `catalog_version`, or a layout the chosen `layout_profile` rejects (e.g. `report` without a hero first). | stderr, prefixed `error: invalid spec:` |
| `3` | Citation check failed (only reachable when `--source-root`/`--source-sha` are given): a `Cite` node's `path:lines` ref_id doesn't resolve at that commit. | stderr, prefixed `error: citation check failed:`, one `  - path:lines: message` line per failing citation |

On success, stdout/the output file carries only the rendered HTML -- no log
lines are interleaved with it.

**Citation checking reuses `glance-check`, not a parallel implementation.**
`glance_check::CitationChecker::check_citations` is the same citation-parse-
and-resolve gate `glance`'s own `check` subcommand uses; this binary calls it
directly rather than re-verifying paths itself. It deliberately does *not*
run `glance-check`'s navigation or page-contract checks -- those encode
glance-gen's own page shape (hero-first, breadcrumbs), not a property every
catalog document has.

## Non-Rust caller proof

`scripts/producer_cli_smoke.py` round-trips a spec to HTML through this
exact seam (build the binary, pipe JSON in, read HTML out, check the exit
code) with no Rust dependency of its own -- the live proof this contract is
actually reachable from outside the Cargo workspace. Run it after
`cargo build -p glance-catalog --bin glance-catalog`:

```sh
python3 scripts/producer_cli_smoke.py
```

## What's not in scope here

- An HTTP surface (the card's own "smallest honest shape" note scopes this
  to a CLI; an HTTP seam is a separate future card if a producer needs one).
- Prompting a model to emit conformant JSON (aesthetic-929's prompt kit).
- Deciding component order for a specific consumer's document shape (each
  producer picks `stream` or `report`, or composes its own
  `glance_catalog::profile::LayoutProfile` in-process if it's a Rust
  consumer).

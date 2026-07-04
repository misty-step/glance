---
prompt_version: glance-007-interior-v1
tier: interior
---
# Mission

Return one JSON object matching the Glance page-spec catalog. Do not return
HTML, Markdown fences, comments, or explanatory prose.

# Catalog

{{catalog_prompt}}

# Reader

Write for someone who has never seen this repository. The hero summary starts
at 10,000 feet in plain language; do not open with crate names, paths, or
jargon unless you gloss them immediately.

# Context Packet

{{context_packet}}

# Interior Contract

- `hero` first, then `narrative` or `flow_diagram`, then any earned story
  components, then exactly one `file_table`, then final `disclosure` context.
- Use `flow_diagram` when child cooperation, gates, or data movement are easier
  to understand as a sequence. Declare nodes and edges only; Rust draws SVG.
- `file_table.rows` covers local files and direct child directories. Dir rows
  link to child pages by name; file rows include exact `signatures[]` from the
  File signatures block.
- Each `role` is one clause, 12 words or fewer.
- Use `callouts` for seams, invariants, contracts, and hurt-you edges.
- `custom_html` is allowed only for an earned interactive miniature. Default to
  no custom HTML.

# Citation Discipline

Use `cite` inline nodes for factual prose. Cite only line ranges that appear in
the context packet, including citations carried forward from child pages. Never
cite context metadata, child labels, prompt instructions, or signatures unless
the same fact is supported by source lines. Never write visible bracket
citations like `[src/lib.rs:1-4]`.

# Story

Do not summarize children one by one. Explain how the children cooperate, what
local files bind them, which invariant is load-bearing, and where a maintainer
can get hurt. Prefer one coherent narrative over many disconnected facts.

# Voice

Plain, confident, concrete. No hedging and no recommendations.

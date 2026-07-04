---
prompt_version: glance-007-leaf-v1
tier: leaf
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

# Leaf Contract

- `hero` first, then `narrative`, then optional `callouts`, then exactly one
  `file_table`, then final `disclosure` context.
- `callouts` is always `{ "type": "callouts", "items": [...] }`; never put
  `kind`, `title`, or `body` directly on the component.
- `disclosure.children` may contain narrative, callouts, flow, file_table,
  image_figure, or custom_html only; never put `hero` or nested `disclosure`
  inside a disclosure.
- Leaf pages never request images and never use `custom_html`.
- `file_table.rows` covers local files. Leaf directories have no child
  directories; if the context says empty, say so plainly and do not invent
  purpose.
- Fill `signatures[]` only with exact text from the File signatures block.
- Each `role` is one clause, 12 words or fewer.

# Citation Discipline

Use `cite` inline nodes for factual prose. Cite only line ranges that appear in
the context packet. Never cite context metadata, directory labels, prompt
instructions, or signatures unless the same fact is supported by source lines.
Never write visible bracket citations like `[src/lib.rs:1-4]`.

# Story

Do not enumerate files as inventory. Explain what this leaf is for, how it
serves the parent or repository, which invariant matters, and what can hurt a
maintainer. If no sharp edge is supported, say that in one cited sentence.

# Voice

Plain, confident, concrete. No hedging and no recommendations.

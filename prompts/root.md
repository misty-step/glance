---
prompt_version: glance-007-root-v1
tier: root
---
# Mission

Return one JSON object matching the Glance page-spec catalog. Do not return
HTML, Markdown fences, comments, or explanatory prose.

# Catalog

{{catalog_prompt}}

# Reader

This is the front door of the generated Glance site. Write for someone who has
never seen this repository. The hero summary starts at 10,000 feet in plain
language; do not open with crate names, paths, or jargon unless you gloss them
immediately.

# Context Packet

{{context_packet}}

# Root Contract

- `hero` first, then `narrative` or `flow_diagram`, then any earned story
  components, then exactly one `file_table`, then final `disclosure` context.
- Root hero may include one `image_request` when the system shape warrants it.
  Emit only `{ "intent": "...", "emphasis": [...] }`; code composes the final
  image prompt from the real source inventory and flow edges.
- Use `flow_diagram` for the central source-to-page or data/control flow.
  Declare nodes and edges only; Rust draws SVG.
- `file_table.rows` covers root files and each real top-level directory once.
  Never recall directories from memory.
- File rows include exact `signatures[]` from the File signatures block. Dir
  rows use empty `signatures`.
- Each `role` is one clause, 12 words or fewer.
- Use `callouts` for seams, invariants, contracts, and hurt-you edges. Its
  shape is always `{ "type": "callouts", "items": [...] }`; never put `kind`,
  `title`, or `body` directly on the component.
- `disclosure.children` may contain narrative, callouts, flow, file_table,
  image_figure, or custom_html only; never put `hero` or nested `disclosure`
  inside a disclosure.
- `custom_html` is allowed only for an earned interactive miniature. Default to
  no custom HTML.

# Citation Discipline

Use `cite` inline nodes for factual prose. Cite only line ranges that appear in
the context packet, including citations carried forward from child pages. Never
cite context metadata, child labels, prompt instructions, workflow-name lists,
or signatures unless the same fact is supported by source lines. Never write
visible bracket citations like `[src/lib.rs:1-4]`.

# Story

Do not list directories and restate their pages. Synthesize the whole: what the
repository is for, how the major directories cooperate, what design choice would
surprise a newcomer, which invariant is load-bearing, and how source becomes a
generated site. Root narrative should feel like a story, not an index.

# Voice

Plain, confident, concrete. No hedging and no recommendations.

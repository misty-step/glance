---
prompt_version: glance-005-interior-v2
tier: interior
---
# Mission

You generate one self-contained HTML Glance page for an interior directory.
Return raw HTML only. The first byte must begin the HTML document:
`<!doctype html>` or `<html`. Do not wrap the response in Markdown fences,
JSON, comments before the HTML, or explanatory prose.

# Reader

Write for someone who will never open the code. This page must explain how the
children cooperate, where the contracts are, and what can hurt an operator or
maintainer.

# Context Packet

{{context_packet}}

# Architecture, Not Inventory

Banned move: re-summarizing the local files or child pages one by one.
Required move: narrate composition. Explain how child directories cooperate,
which local files bind them, what design choice would surprise a newcomer, and
which invariant is load-bearing.

# Citation Discipline

Every factual claim must carry `data-glance-cite="path:start-end"` on the
claiming element. Cite only line ranges that are present in the context packet,
including citations carried forward from child page distillations. Use honest
ranges: the cited lines must directly support the sentence. If a claim has no
citation, do not write it.

Valid citation paths are source file paths from the context packet, such as
`src/lib.rs:1-4`. Never cite context metadata (`directory`, `kind`,
`source_sha`, `one_liner`, parent-chain entries, child directory labels, or
prompt instructions). Do not put `data-glance-cite` on bare path labels, code
elements, headings, citation badges, or decorative text. Put it on the
paragraph, list item, or table row whose factual sentence is directly supported
by the cited source lines.

# Ancestor Constraints

Do not speculate about flags, behavior, runtime modes, side effects,
configuration, environment variables, dependencies, performance, deployment, or
architecture details that are not evidenced by supplied context. Do not provide
recommendations, next steps, or refactor advice. If an empty child page is
present, treat it as a stub and never invent purpose for it.

# Page Contract

Use this exact semantic section contract:

1. `<section class="glance-section" data-glance-section="what-this-is">`
2. `<section class="glance-section" data-glance-section="role-in-the-whole">`
3. `<section class="glance-section" data-glance-section="composition">`
4. `<section class="glance-section" data-glance-section="seams-contracts">`
5. `<section class="glance-section" data-glance-section="where-it-can-hurt-you">`

Put headings inside each section with class `glance-section-title`. Put cited
claim elements in class `glance-cited`. Wrap the child list in class
`glance-composition`. Child links use class `glance-child-link`; child one-line
roles use class `glance-child-role`. Do not invent alternate section class
names.

Composition must link each child page and give each child a one-line role. The
hurt-you section may say "nothing sharp found" only when the evidence genuinely
shows no sharp edge.

# Earned Extras

Default to no diagram and no interactive. At most one inline SVG diagram is
allowed, and only when shape carries meaning prose cannot. At most one small
self-contained interactive is allowed, and only when operating it teaches
behavior. Decorative widgets are forbidden.

# Voice

Plain, confident, concrete. Second person is allowed. Do not hedge with
"appears to", "seems", or "likely".

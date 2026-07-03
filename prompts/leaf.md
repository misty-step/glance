---
prompt_version: glance-005-leaf-v2
tier: leaf
---
# Mission

You generate one self-contained HTML Glance page for a leaf directory.
Return raw HTML only. The first byte must begin the HTML document:
`<!doctype html>` or `<html`. Do not wrap the response in Markdown fences,
JSON, comments before the HTML, or explanatory prose.

# Reader

Write for someone who will never open the code. The page must explain what
this directory is for, how it serves the repository, what design choices matter,
and where the sharp edges are.

# Context Packet

{{context_packet}}

# Architecture, Not Inventory

Banned move: enumerating files with restated contents.
Required move: explain what this directory is for, how it serves the whole,
what design choice would surprise a newcomer, and which invariant is
load-bearing. A leaf narrates only what it can see from the local files,
parent-chain one-liners, and sibling directory names in the context packet.

# Citation Discipline

Every factual claim must carry `data-glance-cite="path:start-end"` on the
claiming element. Cite only line ranges that are present in the context packet.
Use honest ranges: the cited lines must directly support the sentence. If a
claim has no citation, do not write it. Do not cite paths, flags, behavior, or
files that were not provided.

Valid citation paths are source file paths from the context packet, such as
`docs/guide.md:1-3`. Never cite context metadata (`directory`, `kind`,
`source_sha`, `one_liner`, parent-chain entries, sibling names, or prompt
instructions). Do not put `data-glance-cite` on bare path labels, code elements,
headings, citation badges, or decorative text. Put it on the paragraph, list
item, or table row whose factual sentence is directly supported by the cited
source lines.

# Ancestor Constraints

Do not speculate about flags, behavior, runtime modes, side effects,
configuration, environment variables, dependencies, performance, deployment, or
architecture details that are not evidenced by supplied context. Do not provide
recommendations, next steps, or refactor advice. If the context says this is an
empty directory, output the empty-directory stub and no narration.

# Page Contract

Use this exact semantic section contract:

1. `<section class="glance-section" data-glance-section="what-this-is">`
2. `<section class="glance-section" data-glance-section="role-in-the-whole">`
3. `<section class="glance-section" data-glance-section="composition">`
4. `<section class="glance-section" data-glance-section="seams-contracts">`
5. `<section class="glance-section" data-glance-section="where-it-can-hurt-you">`

Put headings inside each section with class `glance-section-title`. Put cited
claim elements in class `glance-cited`. If you mention child composition, wrap
it in class `glance-composition`; a leaf must say there are no child
directories. Do not invent alternate section class names.

Composition must say that there are no child directories for a leaf. The
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

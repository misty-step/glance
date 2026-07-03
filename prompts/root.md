---
prompt_version: glance-005-root-v2
tier: root
---
# Mission

You generate the root HTML page for a Glance site. Return raw HTML only. The
first byte must begin the HTML document: `<!doctype html>` or `<html`. Do not
wrap the response in Markdown fences, JSON, comments before the HTML, or
explanatory prose.

# Reader

Write for someone who will never open the code. This is the front door of the
repository: it must explain the whole system, its major flows, its data model,
and the failure edges aggregated from child pages.

# Context Packet

{{context_packet}}

# Architecture, Not Inventory

Banned move: listing directories and restating their pages.
Required move: synthesize the whole. Explain what the repository is for, how
the major directories cooperate, what design choice would surprise a newcomer,
which invariant is load-bearing, and how the source becomes the generated site.

# Citation Discipline

Every factual claim must carry `data-glance-cite="path:start-end"` on the
claiming element. Cite only line ranges that are present in the context packet,
including citations carried forward from child pages. Use honest ranges: the
cited lines must directly support the sentence. If a claim has no citation, do
not write it.

Valid citation paths are source file paths from the context packet, such as
`README.md:1-3` or `src/lib.rs:1-4`. Never cite context metadata (`directory`,
`kind`, `source_sha`, `one_liner`, child directory labels, workflow-name lists,
or prompt instructions). Do not put `data-glance-cite` on bare path labels,
code elements, headings, citation badges, or decorative text. Put it on the
paragraph, list item, or table row whose factual sentence is directly supported
by the cited source lines.

# Ancestor Constraints

Do not speculate about flags, behavior, runtime modes, side effects,
configuration, environment variables, dependencies, performance, deployment, or
architecture details that are not evidenced by supplied context. Do not provide
recommendations, next steps, or refactor advice. Empty-directory stubs stay
stubs; never narrate them into architecture.

# Page Contract

Use this exact semantic section contract:

1. `<section class="glance-section" data-glance-section="what-this-is">`
2. `<section class="glance-section" data-glance-section="role-in-the-whole">`
3. `<section class="glance-section" data-glance-section="composition">`
4. `<section class="glance-section" data-glance-section="seams-contracts">`
5. `<section class="glance-section" data-glance-section="where-it-can-hurt-you">`
6. `<section class="glance-section" data-glance-section="flows">`
7. `<section class="glance-section" data-glance-section="data-model">`
8. `<section class="glance-section" data-glance-section="failure-edge-index">`

Put headings inside each section with class `glance-section-title`. Put cited
claim elements in class `glance-cited`. Wrap the child list in class
`glance-composition`. Child links use class `glance-child-link`; child one-line
roles use class `glance-child-role`. Do not invent alternate section class
names.

Composition must link each child page and give each child a one-line role.
Flows must trace two to four primary user or data flows across directories,
with every hop cited. Data model must distinguish stored shapes from derived
shapes. Failure-edge index must deduplicate and cite every child hurt-you edge.

# Earned Extras

Default to no diagram and no interactive. At most one inline SVG diagram is
allowed, and only when shape carries meaning prose cannot. At most one small
self-contained interactive is allowed, and only when operating it teaches
behavior. Decorative widgets are forbidden.

# Voice

Plain, confident, concrete. Second person is allowed. Do not hedge with
"appears to", "seems", or "likely".

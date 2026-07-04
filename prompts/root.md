---
prompt_version: glance-006-root-v1
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

# Navigation Contract

The context packet includes a `Navigation` section with repo name, own path,
parent path, breadcrumb coordinates, ordered sibling directories, ordered child
directories, and the exact relative hrefs to use. Every page must include a
visible `<nav class="glance-nav">` in the header. It must contain:

- `.glance-breadcrumb`: root -> ... -> current page. Link every ancestor with
  the exact href from the Navigation packet.
- Parent link: every non-root page links to its parent. Root has no parent.
- `.glance-nav-children`: link every direct child directory with the exact
  child hrefs from the Navigation packet.
- `.glance-nav-siblings`: either a compact sibling row or prev/next sibling
  links using the exact sibling hrefs.

Put `data-glance-directory` on the `.glance-page` body or shell. Use `.` for
the root page. Relative links are machine-checked.

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

Citation attributes are machine-checked before the page is written. Use the
canonical grammar `path:start-end[,start-end...][,path:start-end...]`: each
comma-separated segment is either `path:start-end` or a bare `start-end` range
that inherits the previous path, including citations carried forward from child
pages.

GOOD:
`<p class="glance-cited" data-glance-cite="README.md:1-3">The README defines the project purpose.</p>`
`<p class="glance-cited" data-glance-cite="README.md:1-3,5-7">The README defines two linked facts.</p>`
`<p class="glance-cited" data-glance-cite="README.md:1-3,src/lib.rs:4-6">Two files define the project shape.</p>`

BAD:
`<p class="glance-cited" data-glance-cite="1-3">The README defines the project purpose.</p>`
`<p class="glance-cited" data-glance-cite="1-3,README.md:4-6">The first segment has no path to inherit.</p>`

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

# Visual Register

Start from a calm, dense reference-documentation register: two-column sticky
sidebar nav when the page has enough sections, restrained neutral palette plus
one accent color, monospace citation badges on every factual claim, generous
section rhythm, and real HTML tables or definition lists instead of prose walls
for structured facts.

Extend that register with the operator-ratified flow-lanes pattern. Define one
shared visual vocabulary of actors: the system entry points, roles, gates,
datastores, source roots, generated pages, publisher boundaries, or UI surfaces
that matter to the repository. Assign each actor one fixed accent color plus
one small line-icon glyph using CSS custom properties. Reuse the exact same
actor color and icon in both:

1. Composition cards that are real clickable navigation cards: icon, bold
   link-like label, one-line role description, trailing "view ->" cue on hover,
   and any factual role sentence cited inline.
2. Swimlane diagrams for the two or three flows whose behavior is genuinely
   easier to understand as a cross-actor sequence than as prose. Build them as
   CSS grids of lane columns; each lane header reuses the actor's icon/color
   pair; each lane body is a vertical stack of small step cards threaded by a
   thin rail line; a subtle dot may animate along the rail to show direction.

Do not force every flow into a lane. Symmetric, instantaneous, or simple flows
read better as short bordered prose blocks with a monospace tag. Choose the
treatment based on whether sequence across actors is the hard thing to convey.

# Theme Contract

Light, dark, and system theming are required on every page. Implement them with
CSS custom properties only: define the full color palette, including every
actor accent/background/border triplet, once in a light `:root` block; mirror
every token inside both `@media (prefers-color-scheme: dark)` and
`[data-theme="dark"]`; provide `[data-theme="light"]` overrides. Add a tiny
three-way control for light/dark/auto that writes explicit choices to
`localStorage` and clears the stored value for auto. Never hardcode colors,
borders, shadows, or diagram lines outside the token set.

# Earned Extras

Pages should lean into the medium: animated SVG flow diagrams where flows
exist, visual hierarchy over prose walls, and illustrative architecture instead
of merely descriptive text. CSS/SVG animation is encouraged when it carries
meaning and must respect `prefers-reduced-motion`. Interactives are allowed
only where behavior carries meaning. Decorative widgets are forbidden.

The root page may request one hero architecture illustration when the system
shape warrants it:

`<figure class="glance-image-request" data-glance-image-prompt="..." data-glance-image-alt="..."></figure>`

Use at most one root hero image request. The prompt must describe an
architecture illustration grounded in cited page content, not a decorative
banner. The image pipeline may render it later; the HTML must remain useful as
a styled figure fallback if no image is available.

# Voice

Plain, confident, concrete. Second person is allowed. Do not hedge with
"appears to", "seems", or "likely".

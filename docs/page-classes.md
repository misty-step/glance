# Generated page class contract

Glance pages are self-contained HTML documents. A provider may choose the visual
style, but these semantic hooks are stable because context assembly and checks
depend on them.

## Document

- `.glance-page` on the page body or main document shell.
- `data-glance-directory` on the `.glance-page` element, using `.` for the
  repository root and repo-relative directory paths elsewhere.
- `.glance-header` for the page title and source metadata.
- `.glance-nav` for the deterministic page navigation header.
- `.glance-breadcrumb` for the root-to-current breadcrumb links.
- `.glance-nav-children` for child directory links.
- `.glance-nav-siblings` for sibling directory links or prev/next links.
- `.glance-section` on every contract section.
- `.glance-section-title` on visible section headings.
- `.glance-cited` on elements that make factual claims and carry
  `data-glance-cite`.

`data-glance-cite` uses the canonical grammar
`path:start-end[,start-end...][,path:start-end...]`. Each comma-separated
segment is either a repo-relative `path:start-end` citation or a bare
`start-end` range that inherits the previous path. The first segment must name
a path.

## Required sections

Every page must include one element for each section below. Use both the class
and `data-glance-section` value so parsers have a stable target.

| Section | Required selector |
| --- | --- |
| What this is | `.glance-section[data-glance-section="what-this-is"]` |
| Role in the whole | `.glance-section[data-glance-section="role-in-the-whole"]` |
| Composition | `.glance-section[data-glance-section="composition"]` |
| Seams and contracts | `.glance-section[data-glance-section="seams-contracts"]` |
| Where it can hurt you | `.glance-section[data-glance-section="where-it-can-hurt-you"]` |

Root pages must also include:

| Section | Required selector |
| --- | --- |
| Flows | `.glance-section[data-glance-section="flows"]` |
| Data model | `.glance-section[data-glance-section="data-model"]` |
| Failure-edge index | `.glance-section[data-glance-section="failure-edge-index"]` |

## Composition hooks

- `.glance-composition` wraps the child directory list.
- `.glance-child-link` is the link to a child `index.html`.
- `.glance-child-role` is the one-line role text for that child.

## Navigation hooks

Every generated page must include a visible navigation header before the main
section flow:

- Breadcrumb: root to current page. The current page may be plain text; every
  ancestor must be a relative link to its `index.html`.
- Parent: every non-root page must link to its parent page.
- Children: every child directory in the source tree must link to that child's
  generated `index.html`.
- Siblings: use either a compact row or prev/next links. These are required by
  the prompt contract for usability; deterministic validation currently gates
  parent and child links because those are the structural spine.

Relative hrefs must resolve from the current page's output directory. Examples:

| Page | Target | Required href shape |
| --- | --- | --- |
| `.` | `docs` child | `docs/index.html` |
| `src` | root parent | `../index.html` |
| `src` | `src/parser` child | `parser/index.html` |
| `src/parser` | `src` parent | `../index.html` |
| `docs` | `src` sibling | `../src/index.html` |

`glance check` validates that every generated page has `data-glance-directory`,
has the required parent link except at root, and links to each child directory
known from the source snapshot.

## Theme contract

Every page must support light, dark, and system themes with CSS custom
properties only. Define tokens in `:root`, mirror them in
`@media (prefers-color-scheme: dark)`, and provide `[data-theme="dark"]` plus
`[data-theme="light"]` overrides. The page must include a small three-way
control for `light`, `dark`, and `auto`; explicit choices are stored in
`localStorage`, while `auto` clears the stored choice.

Tiny reference snippet for templates:

```html
<style>
  :root {
    color-scheme: light dark;
    --glance-bg: #f7f5ef;
    --glance-text: #171614;
    --glance-line: #d8d2c4;
    --glance-accent: #2457d6;
  }
  @media (prefers-color-scheme: dark) {
    :root {
      --glance-bg: #111316;
      --glance-text: #f1efe7;
      --glance-line: #343941;
      --glance-accent: #9ab3ff;
    }
  }
  [data-theme="light"] {
    --glance-bg: #f7f5ef;
    --glance-text: #171614;
    --glance-line: #d8d2c4;
    --glance-accent: #2457d6;
  }
  [data-theme="dark"] {
    --glance-bg: #111316;
    --glance-text: #f1efe7;
    --glance-line: #343941;
    --glance-accent: #9ab3ff;
  }
</style>
```

## Earned extras

- `.glance-diagram` marks an inline SVG diagram.
- `.glance-interactive` marks the single allowed self-contained interactive.
- `.glance-image` marks a rendered image figure.
- `.glance-image-request` marks a pending image request fallback.

Pages should lean into the HTML medium when the source warrants it: animated SVG
flow diagrams for flows, compact visual hierarchy instead of prose walls, and
illustrations where architecture is easier to recognize visually than read.
Animation must respect `prefers-reduced-motion`. Interactives are valid only
when behavior carries meaning, not as decoration.

Generated pages may request one or more image renderings with:

```html
<figure
  class="glance-image-request"
  data-glance-image-prompt="Create a clear architecture illustration..."
  data-glance-image-alt="Architecture illustration of the repository shape">
</figure>
```

The image pipeline rewrites successful requests to an `<img>` written beside
the page. If rendering fails or the per-run image budget is exhausted, the page
keeps a styled figure with the requested alt text and never emits a broken
image URL.

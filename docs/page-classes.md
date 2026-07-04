# Rendered page contract

Glance models emit JSON page specs. They do not emit raw HTML. Rust validates
the spec against `glance-catalog-001` and renders a self-contained HTML page
with the repo-owned kit.

## Document

- `<html data-glance-catalog-version="glance-catalog-001">`
- `data-source-sha` and `data-prompt-version` on the HTML element.
- `.glance-page[data-glance-directory]` on the body, using `.` for root.
- `.glance-nav` with breadcrumb, parent, child, and sibling links assembled
  from the source snapshot.
- `.glance-main > [data-glance-component]` for top-level catalog components.

## Component order

Top-level components must progressively disclose the page:

1. `hero`
2. `narrative` or `flow_diagram`
3. other earned story components: `flow_diagram`, `callouts`, `image_figure`,
   or `custom_html`
4. exactly one `file_table`
5. `disclosure` components last

`glance check` validates this order for catalog-rendered pages and rejects
visible bracket citation noise such as `[src/lib.rs:1-4]`.

## Citations

Specs express factual evidence as inline `cite` nodes with `text`, `path`, and
`lines`. The renderer turns those phrases into subtle links with
`data-glance-cite`; visible bracket citations are never part of body copy.

`data-glance-cite` uses the canonical grammar
`path:start-end[,start-end...][,path:start-end...]`. Each comma-separated
segment is either a repo-relative `path:start-end` citation or a bare
`start-end` range that inherits the previous path. The first segment must name
a path.

## Navigation

Every generated page includes a visible navigation header before the main
component flow:

- Breadcrumb: root to current page.
- Parent: every non-root page links to its parent page.
- Children: every direct child directory links to that child's generated page.
- Siblings: sibling links are included when present.

Relative hrefs resolve from the current page's output directory. Examples:

| Page | Target | Required href shape |
| --- | --- | --- |
| `.` | `docs` child | `docs/index.html` |
| `src` | root parent | `../index.html` |
| `src` | `src/parser` child | `parser/index.html` |
| `src/parser` | `src` parent | `../index.html` |
| `docs` | `src` sibling | `../src/index.html` |

## Theme and media

The kit owns light, dark, and system themes with CSS custom properties and a
small three-way control. Diagrams are renderer-owned animated SVG and respect
`prefers-reduced-motion`.

Models may request images only through structured `image_request` fields.
`docs/images.md` defines eligibility and prompt composition. The image pipeline
rewrites successful requests to `.glance-image` figures and keeps a responsive
fallback when rendering fails or budget is exhausted.

`custom_html` is the only raw HTML escape hatch. It is limited to one bounded,
sandboxed iframe on interior/root pages; citations declared for it are still
checked.

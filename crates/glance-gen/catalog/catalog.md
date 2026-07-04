# Glance Page Spec Catalog v001

Return a single JSON object that matches `glance-catalog-001`. Do not return
HTML. The model chooses content and structure inside this catalog; Rust renders
the page chrome, navigation, citations, diagrams, images, theme behavior, and
responsive layout.

Top-level shape:

```json
{
  "catalog_version": "glance-catalog-001",
  "title": "Page title",
  "components": []
}
```

Progressive disclosure is mandatory:

1. `hero` is first.
2. `narrative` or `flow_diagram` follows immediately.
3. Other story components (`callouts`, `image_figure`, or `custom_html`) may
   follow when earned.
4. `file_table` is required once on every page after story components.
5. `disclosure` components are last.

Components:

- `hero`: `title`, one plain-language `summary` paragraph as inline nodes, two
  to four `stats`, optional `image_request` for root pages only.
- `narrative`: `heading`, `paragraphs`; each paragraph is an array of inline
  nodes.
- `flow_diagram`: `nodes`, `edges`, optional `lanes`; Rust draws animated SVG.
- `file_table`: enhanced file tree rows with `name`, `kind`, `role`,
  `signatures`, optional `gotcha`, optional `cite`. Every row includes
  `kind` and `signatures`; directories use an empty signature array.
- `callouts`: `kind` of `seam`, `hurt`, `invariant`, or `contract`; each item
  has `title` and inline-node `body`.
- `disclosure`: collapsed `heading` with child components for full context.
  Children may not be `hero` or nested `disclosure`.
- `image_figure`: structured `image_request`; never a raw URL.
- `custom_html`: one earned, bounded interactive for interior/root pages only.
  Declare any citations inside `citations` so deterministic checking can still
  validate them.

Inline nodes:

- `text`: `{ "type": "text", "text": "plain words" }`
- `cite`: `{ "type": "cite", "text": "the cited phrase", "path": "src/lib.rs", "lines": "1-4" }`
- `link`: `{ "type": "link", "text": "label", "href": "https://example.com" }`

Citation text must read as prose. Never write visible bracket citations such as
`[src/lib.rs:1-4]`; the renderer turns cite nodes into subtle source links and
popovers.

Important object shapes:

```json
{
  "type": "callouts",
  "items": [
    {
      "kind": "hurt",
      "title": "What can hurt you",
      "body": [{ "type": "text", "text": "Plain prose." }]
    }
  ]
}
```

Never put `kind`, `title`, or `body` directly on the `callouts` component; they
belong inside `items[]`.

```json
{
  "type": "file_table",
  "rows": [
    {
      "name": "src/lib.rs",
      "kind": "file",
      "role": "Exports the crate API.",
      "signatures": ["pub fn render() -> Result<()>"]
    }
  ]
}
```

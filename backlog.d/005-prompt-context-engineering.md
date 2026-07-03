# 005 prompt/context engineering per harness-engineering practices

Status: pending

## Goal

Design leaf, interior, and root prompts plus context assembly so generated pages
use child summaries, parent/sibling context, and pinned source snippets without
forcing rigid schema where model-native prose is the product.

## Oracle

- Prompt fixtures show the exact context packet for a leaf, interior package,
  and root page.
- Held-out generation smoke tests produce cited, non-file-listing prose.
- Context assembly never reads or writes generated HTML inside the source repo.

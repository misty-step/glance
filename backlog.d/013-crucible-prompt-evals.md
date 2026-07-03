# 013 Crucible prompt evals

Status: pending

## Goal

Route prompt-quality evaluation through Crucible so leaf, interior, and root
prompt versions can be compared against held-out fixture trees without adding
LLM judges to `glance check`.

## Oracle

- Crucible records prompt version, fixture tree, provider/model, citation-check
  result, and page-structure result for each run.
- A small smoke suite covers leaf, interior, and root pages.
- `glance check` remains deterministic and does not call model judges.

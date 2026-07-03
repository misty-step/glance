# 002 citation gate

Status: done

## Goal

Ship the deterministic honesty gate for generated pages: parse citation spans
from HTML and verify that every cited file and line range exists at the pinned
source SHA.

## Oracle

- `glance-check` tests include a committed mini source tree plus generated
  pages with good and broken citations.
- `glance check --source-root <repo> --source-sha <sha> <html>` exits zero for
  good pages and non-zero for broken citations.
- No LLM judge participates in the gate.

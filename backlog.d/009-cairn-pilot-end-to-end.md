# 009 cairn pilot end-to-end

Status: pending

## Goal

Run the first complete pilot from `cairn` to `cairn-glance` to deployed site,
using real triggers, real generated pages, citation checking, and site serving.

## Oracle

- A source PR in `cairn` produces a mirrored `cairn-glance` PR.
- Merge to `cairn` master updates `cairn-glance` master and the deployed site.
- The pilot report includes costs, changed directories, check results, and
  operator-facing site URL.

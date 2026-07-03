# 001 walker + hash + plan

Status: done

## Goal

Ship the deterministic source-tree substrate: walk a repository, compute
per-directory content hashes, resolve a pinned source SHA, and produce the
leaf-to-root regeneration plan for changed paths or changed snapshots.

## Oracle

- `glance-core` tests cover a mini source tree, changed-path planning, and
  snapshot hash deltas.
- `glance plan --root <fixture> --changed <path>` prints the changed directory
  path plus ancestors, leaf to root.
- `scripts/check.sh` passes locally and in GitHub Actions.

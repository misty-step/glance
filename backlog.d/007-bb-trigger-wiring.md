# 007 bb trigger wiring

Status: pending

## Goal

Wire bitterblossom source-repo webhooks so source PRs and merges trigger the
right glance run, plan scope, and reporting path.

## Oracle

- Webhook fixtures for PR open/update and merge produce the expected run plan.
- A dry-run posts the mirrored PR link back to the source PR.
- Merge events update the sister master path and emit a canary-readable result.

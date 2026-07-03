# 011 canary + landmark + powder citizenship

Status: pending

## Goal

Make glance a normal Misty Step fleet citizen: run failures and freshness report
to canary, releases go through landmark, and work state is tracked in powder.

## Oracle

- Failed generation/check runs produce a canary event with source repo, SHA,
  phase, and failure class.
- A release path exists through landmark with a reproducible artifact.
- Powder carries the active work item and closure evidence.

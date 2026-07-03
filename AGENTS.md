# glance agent notes

- Work in Rust. Real provider calls, publisher plumbing, and site serving
  infrastructure are deferred unless a ticket explicitly promotes them.
- The repo-owned gate is `scripts/check.sh`; hosted CI must call that script
  instead of duplicating gate logic.
- Generated HTML belongs in sister repos, never in a source repository.
- Citation checking is deterministic. Do not add LLM judges to `glance check`.
- Tests should exercise observable behavior through crate APIs and CLI paths.

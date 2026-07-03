# 006 sister-repo publisher

Status: pending

## Goal

Implement git plumbing for `<name>-glance`: initialize or clone the sister repo,
write only generated HTML and metadata, commit changes, and open/update the
mirrored PR.

## Oracle

- A fixture source repo publishes to a fixture bare sister repo with matching
  directory structure and only generated files.
- Source PR runs open a sister PR and report the URL.
- Merge-to-master updates sister master without writing to the source repo.

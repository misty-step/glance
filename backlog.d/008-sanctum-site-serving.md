# 008 Sanctum site serving

Status: pending

## Goal

Serve sister-repo HTML sites through Sanctum as the deployed site of record for
master, with stable routes and cache-safe updates.

## Oracle

- A generated sister repo can be served locally and through Sanctum.
- Master updates are visible at the canonical site URL after the sister repo
  update.
- Stale site detection fails if the served SHA lags source master.

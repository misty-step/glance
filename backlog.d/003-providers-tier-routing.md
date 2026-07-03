# 003 providers + tier routing

Status: pending

## Goal

Implement real generation providers for OpenRouter and Gemini with depth tiers,
per-repo daily budgets, spend reporting, retry policy, and provider-specific
error classification.

## Oracle

- Provider tests run against recorded/emulated responses without leaking keys.
- A dry-run reports the tier selected for leaf, interior, and root pages.
- A budget-exhausted run fails closed before provider calls are made.

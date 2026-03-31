# Contradiction Prepare/Commit — Not Yet in Enterprise

**Status:** Parked  
**Date:** 2026-03-31

## Current State

- **Core engine** (`hebbs-core/src/contradict.rs`): Full two-phase pipeline exists.
  - `check_memory_contradictions()` — auto-detection on remember (LLM + heuristic)
  - `prepare_contradictions()` — retrieves pending contradictions for review
  - `commit_contradictions()` — accepts verdicts (contradiction / revision / dismiss)
  - Creates CONTRADICTS and REVISED_FROM edges with confidence + timestamp metadata.

- **CLI** (`hebbs contradiction-prepare`, `hebbs contradiction-commit`): Works locally.

- **Enterprise** (`hebbs-platform`): Only a config toggle (`contradiction.enabled: true`).
  No REST endpoints for prepare/commit. No dashboard UI for reviewing contradictions.

## What's Missing for Enterprise

1. **REST endpoints**: `POST /v1/workspaces/:slug/contradictions/prepare` and `POST /v1/workspaces/:slug/contradictions/commit`
2. **Dashboard UI**: Review pending contradictions, approve/dismiss with one click
3. **Notification**: Alert team when contradictions are detected (email, Slack webhook)
4. **Bulk operations**: Review multiple contradictions at once

## When to Build

When a specific vertical use case demands it:
- CRM: pipeline contradictions ("forecast says Commit but no meeting in 3 weeks")
- Legal: conflicting clauses across documents
- Finance: policy vs practice discrepancies
- Compliance: audit trail conflicts

Until then, contradictions are auto-detected in the background (creates CONTRADICTS edges) but the human review flow is CLI-only.

## For Guides

The default guide and vertical guides should mention contradiction **detection** as a feature (it works automatically). The prepare/commit **review flow** should be shown as CLI-only for now. Don't promise dashboard-based contradiction review in enterprise guides until endpoints + UI exist.

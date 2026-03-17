# Incident Log

## INC-2024-031 — Search outage (P0) — 2024-03-12

**Duration:** 47 minutes (14:03–14:50 UTC)
**Impact:** 100% of search requests failed. Approximately 22,000 affected users.
**Root cause:** Redis cluster failover triggered by a routine maintenance restart. The search service was not configured to retry on failover and returned 500s instead of falling back to PostgreSQL.
**Fix:** Added retry logic with exponential backoff. Failover now transparent to users.
**Owner:** Alice Chen
**Postmortem:** Completed 2024-03-14

## INC-2024-019 — Slow dashboard (P1) — 2024-02-28

**Duration:** 3 hours (09:00–12:00 UTC)
**Impact:** Dashboard load times degraded to 18s (normal: <2s) for tenants with >10K users.
**Root cause:** Missing index on `tenant_id` column in the `events` table following a schema migration.
**Fix:** Added index. Query time dropped from 12s to 45ms.
**Owner:** Alice Chen
**Postmortem:** Completed 2024-03-01

## INC-2024-008 — Data pipeline lag (P1) — 2024-01-15

**Duration:** 6 hours
**Impact:** Analytics dashboards showed data 6 hours stale. No data loss.
**Root cause:** Disk full on the TimescaleDB node due to unrotated logs.
**Fix:** Added log rotation, disk monitoring alert at 75% threshold.
**Owner:** Carol Singh

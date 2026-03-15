# HEBBS Operations Runbook

> **Audience:** On-call engineers and SREs operating HEBBS in production.
>
> **HEBBS version:** 0.x (single-binary, embedded RocksDB)
>
> **Ports:** gRPC `:50051` | HTTP `:6381` (metrics at `/v1/metrics`, health at `/v1/health`)

---

## Table of Contents

- [Bare Metal / VM Operations](#bare-metal--vm-operations)
- [Backup and Restore](#backup-and-restore)
- [Scaling Guide](#scaling-guide)
- [Upgrade Procedure](#upgrade-procedure)
- [Incident: Instance Unreachable](#incident-instance-unreachable)
- [Incident: Latency Degradation](#incident-latency-degradation)
- [Incident: Elevated Error Rate](#incident-elevated-error-rate)
- [Incident: Disk Capacity](#incident-disk-capacity)
- [Incident: Reflect Pipeline Stall](#incident-reflect-pipeline-stall)
- [Incident: Authentication Failure Spike](#incident-authentication-failure-spike)
- [Incident: OOM Kill](#incident-oom-kill)

---

## Bare Metal / VM Operations

For deployments outside Kubernetes, HEBBS runs as a systemd service. The install script sets this up automatically with `--with-systemd`.

### File Layout

| Path | Purpose |
|------|---------|
| `/usr/local/bin/hebbs` | Unified binary (server, CLI, vault) |
| `/etc/hebbs/hebbs.toml` | TOML configuration |
| `/etc/hebbs/hebbs.env` | Environment overrides and API keys (mode 600) |
| `/var/lib/hebbs/` | Data directory (RocksDB, models, auth keys) |
| `/etc/systemd/system/hebbs.service` | Systemd unit (`hebbs serve --foreground`) |

### Service Management

```bash
# Start / stop / restart
sudo systemctl start hebbs
sudo systemctl stop hebbs       # sends SIGTERM, waits TimeoutStopSec
sudo systemctl restart hebbs

# Enable auto-start on boot
sudo systemctl enable hebbs

# Check status
sudo systemctl status hebbs
```

### Viewing Logs

Logs go to journald (structured JSON when `logging.format = "json"`):

```bash
# Stream live logs
journalctl -u hebbs -f

# Last 200 lines
journalctl -u hebbs -n 200

# Logs since last boot
journalctl -u hebbs -b

# Filter by severity
journalctl -u hebbs -p err

# Logs within a time window
journalctl -u hebbs --since "2025-01-15 10:00" --until "2025-01-15 12:00"
```

### Configuration Changes

After editing `/etc/hebbs/hebbs.toml` or `/etc/hebbs/hebbs.env`:

```bash
sudo systemctl restart hebbs
```

### Shutdown Timeout

The server enforces `shutdown_timeout_secs` (default 15s) after receiving SIGTERM. If graceful shutdown (connection drain + background worker stop) does not complete within this window, the process force-exits.

The systemd unit sets `TimeoutStopSec=20` — 5 seconds higher than the default — so the application handles its own timeout before systemd sends SIGKILL. If you increase `shutdown_timeout_secs` in `hebbs.toml`, increase `TimeoutStopSec` in the unit file to match:

```bash
sudo systemctl edit hebbs
```

```ini
[Service]
TimeoutStopSec=35
```

### Upgrading on Bare Metal

```bash
# 1. Download the new version
HEBBS_VERSION=v0.3.0 curl -sSf https://hebbs.ai/install | sudo sh

# 2. Restart the service (picks up the new binary)
sudo systemctl restart hebbs

# 3. Verify
sudo systemctl status hebbs
curl -s http://localhost:6381/v1/health/ready
```

### Monitoring Without Prometheus

If Prometheus is not available, use the health and metrics endpoints directly:

```bash
# Health check
curl -s http://localhost:6381/v1/health/ready | jq .

# Prometheus metrics (text format)
curl -s http://localhost:6381/v1/metrics | head -20
```

Set up a cron job or external monitoring tool to poll `/v1/health/ready` and alert on non-200 responses.

---

## Backup and Restore

HEBBS uses RocksDB as its embedded storage engine. Backups are RocksDB checkpoint-based — an atomic, point-in-time snapshot of the database that uses hard links (fast, near-zero I/O cost on the same filesystem).

### Creating a Backup

```bash
# 1. Identify the HEBBS pod
HEBBS_POD=$(kubectl get pods -l app=hebbs -o jsonpath='{.items[0].metadata.name}')

# 2. Create a checkpoint directory inside the container
TIMESTAMP=$(date +%Y%m%d-%H%M%S)
kubectl exec "$HEBBS_POD" -- \
  hebbs backup create --output /data/backups/checkpoint-${TIMESTAMP}

# 3. Verify the checkpoint was created
kubectl exec "$HEBBS_POD" -- ls -la /data/backups/checkpoint-${TIMESTAMP}

# 4. Copy the backup to a persistent location outside the cluster
kubectl cp "$HEBBS_POD":/data/backups/checkpoint-${TIMESTAMP} \
  ./hebbs-backup-${TIMESTAMP}

# 5. Upload to object storage (recommended)
aws s3 cp --recursive ./hebbs-backup-${TIMESTAMP} \
  s3://your-backup-bucket/hebbs/checkpoint-${TIMESTAMP}/
```

### Restoring from Backup

> **Warning:** Restore replaces the entire database. All data written after the checkpoint will be lost.

```bash
# 1. Scale down HEBBS to zero replicas
kubectl scale deployment hebbs --replicas=0
kubectl wait --for=condition=Available=False deployment/hebbs --timeout=60s

# 2. Copy the backup to the data volume
# If using a PVC, create a temporary pod to mount it:
kubectl run hebbs-restore --image=busybox --restart=Never \
  --overrides='{
    "spec": {
      "containers": [{
        "name": "restore",
        "image": "busybox",
        "command": ["sleep", "3600"],
        "volumeMounts": [{"name": "data", "mountPath": "/data"}]
      }],
      "volumes": [{"name": "data", "persistentVolumeClaim": {"claimName": "hebbs-data"}}]
    }
  }'
kubectl wait --for=condition=Ready pod/hebbs-restore --timeout=60s

# 3. Clear the existing database and copy backup in
kubectl exec hebbs-restore -- rm -rf /data/db
kubectl cp ./hebbs-backup-${TIMESTAMP} hebbs-restore:/data/db

# 4. Clean up the restore pod and scale back up
kubectl delete pod hebbs-restore
kubectl scale deployment hebbs --replicas=1
kubectl rollout status deployment/hebbs --timeout=120s

# 5. Verify the restore
curl -s http://localhost:6381/v1/health | jq .
```

### Backup Schedule Recommendation

| Environment | Frequency       | Retention |
|-------------|-----------------|-----------|
| Production  | Every 6 hours   | 7 days    |
| Staging     | Daily           | 3 days    |
| Development | Before upgrades | 1 day     |

---

## Scaling Guide

HEBBS is a single-binary, vertically-scaled service. Horizontal scaling (multiple replicas) requires shared-nothing partitioning at the application layer and is not covered here.

### RAM Sizing Formula

```
RAM_MB ≈ block_cache_mb + (memory_count × 0.00005) + 200
```

Where:
- `block_cache_mb` — RocksDB block cache size (configured via `HEBBS_BLOCK_CACHE_MB`, default 256)
- `memory_count` — total number of stored memories
- `200` — baseline overhead (runtime, HNSW graph, gRPC buffers, OS page cache headroom)

### Sizing Table

| Memory Count | block_cache_mb | Calculated RAM (MB) | Recommended Pod Limit |
|--------------|---------------|---------------------|-----------------------|
| 100,000      | 256           | 461                 | 512 Mi                |
| 500,000      | 512           | 737                 | 1 Gi                  |
| 1,000,000    | 512           | 762                 | 1 Gi                  |
| 2,000,000    | 1024          | 1324                | 2 Gi                  |
| 5,000,000    | 2048          | 2498                | 3 Gi                  |
| 10,000,000   | 4096          | 4796                | 6 Gi                  |

### CPU Guidance

- **2 vCPU** handles most workloads under 1M memories.
- **4 vCPU** recommended for 1M–5M memories or when reflect pipeline runs frequently.
- **8 vCPU** for 5M+ memories or high-throughput ingestion scenarios.

RocksDB background compaction benefits from additional cores. Set `HEBBS_ROCKSDB_MAX_BACKGROUND_JOBS` to `min(cpu_count - 1, 4)`.

### Disk Sizing

```
DISK_GB ≈ (memory_count × avg_memory_bytes) × 2.5 / 1_000_000_000
```

The 2.5x multiplier accounts for RocksDB write amplification, HNSW index overhead, and compaction temporary space. Use SSD/NVMe storage — HDD causes unacceptable compaction latency.

---

## Upgrade Procedure

### Single-Replica Upgrade

```bash
# 1. Create a pre-upgrade backup
HEBBS_POD=$(kubectl get pods -l app=hebbs -o jsonpath='{.items[0].metadata.name}')
kubectl exec "$HEBBS_POD" -- hebbs backup create --output /data/backups/pre-upgrade

# 2. Update the image tag
kubectl set image deployment/hebbs hebbs=ghcr.io/hebbs-ai/hebbs:NEW_VERSION

# 3. Wait for rollout
kubectl rollout status deployment/hebbs --timeout=300s

# 4. Verify health
curl -s http://localhost:6381/v1/health | jq .

# 5. Run a smoke test
grpcurl -plaintext localhost:50051 hebbs.v1.HebbsService/Health
```

### Multi-Replica Upgrade (Rolling)

> **Pre-requisite:** Ensure your deployment has `maxUnavailable: 0` and `maxSurge: 1` in its rolling update strategy so at least N replicas remain available at all times.

```bash
# 1. Create a backup from one replica
HEBBS_POD=$(kubectl get pods -l app=hebbs -o jsonpath='{.items[0].metadata.name}')
kubectl exec "$HEBBS_POD" -- hebbs backup create --output /data/backups/pre-upgrade

# 2. Update the image — Kubernetes will drain one pod at a time
kubectl set image deployment/hebbs hebbs=ghcr.io/hebbs-ai/hebbs:NEW_VERSION

# 3. Monitor the rollout
kubectl rollout status deployment/hebbs --timeout=600s

# 4. If a pod fails readiness, pause the rollout
kubectl rollout pause deployment/hebbs

# 5. Investigate the failing pod
kubectl logs -l app=hebbs --tail=100

# 6. If rollback is needed
kubectl rollout undo deployment/hebbs
kubectl rollout status deployment/hebbs --timeout=300s

# 7. If the issue is fixed, resume
kubectl rollout resume deployment/hebbs
```

### Pre-Upgrade Checklist

- [ ] Read the release notes for breaking changes
- [ ] Backup created and verified
- [ ] Alerting silenced for the maintenance window
- [ ] Monitoring dashboard open (`HEBBS Overview`)
- [ ] Rollback procedure reviewed

---

## Incident: Instance Unreachable

**Alert:** `HebbsDown`

### 1. Symptoms

- `up{job="hebbs"} == 0` — Prometheus cannot scrape the metrics endpoint
- gRPC clients receive connection refused or deadline exceeded
- HTTP health check at `/v1/health` returns no response
- Dashboard shows gaps in all metric panels

### 2. Immediate Triage

```bash
# Is the pod running?
kubectl get pods -l app=hebbs -o wide

# Check pod events
kubectl describe pod -l app=hebbs | tail -30

# Check for OOM kills or restarts
kubectl get pods -l app=hebbs -o jsonpath='{range .items[*]}{.metadata.name}{"\t"}{.status.containerStatuses[0].restartCount}{"\t"}{.status.containerStatuses[0].lastState.terminated.reason}{"\n"}{end}'

# Check recent logs
kubectl logs -l app=hebbs --tail=200 --timestamps

# Check node status
kubectl get nodes -o wide
kubectl describe node $(kubectl get pods -l app=hebbs -o jsonpath='{.items[0].spec.nodeName}')
```

### 3. Root Cause Analysis

Common causes, ranked by likelihood:

1. **OOM Kill** — Pod exceeded memory limits. Check `lastState.terminated.reason` for `OOMKilled`. See [Incident: OOM Kill](#incident-oom-kill).
2. **Crash loop** — Application panic or fatal error. Check logs for panic backtraces or `FATAL` entries.
3. **Node failure** — The underlying node is `NotReady`. Check `kubectl get nodes`.
4. **Disk full** — RocksDB cannot open if the data directory's filesystem is full. Check `df -h` on the node or PVC usage.
5. **Configuration error** — Bad environment variable or missing secret after a deploy. Check `kubectl describe pod` for init container or volume mount failures.
6. **Network partition** — Prometheus can't reach the pod, but the pod may be running. Check from inside the cluster: `kubectl run debug --image=busybox --rm -it -- wget -qO- http://hebbs:6381/v1/health`.

### 4. Resolution

**OOM Kill:**
```bash
# Increase memory limits
kubectl patch deployment hebbs -p '{"spec":{"template":{"spec":{"containers":[{"name":"hebbs","resources":{"limits":{"memory":"2Gi"}}}]}}}}'
kubectl rollout status deployment/hebbs
```

**Crash loop:**
```bash
# Check the exit code and logs
kubectl logs -l app=hebbs --previous --tail=500
# If the issue is a corrupt database, restore from backup (see Backup and Restore)
```

**Disk full:**
```bash
# See Incident: Disk Capacity
```

**Configuration error:**
```bash
# Compare running config vs expected
kubectl get deployment hebbs -o yaml | grep -A 20 env
# Fix the config and redeploy
```

### 5. Verification

```bash
# Confirm pod is running and ready
kubectl get pods -l app=hebbs
# Response should show 1/1 READY, STATUS Running

# Confirm metrics endpoint responds
curl -s http://localhost:6381/v1/metrics | head -5

# Confirm gRPC is healthy
grpcurl -plaintext localhost:50051 hebbs.v1.HebbsService/Health

# Confirm Prometheus target is UP in the Grafana dashboard
```

### 6. Prevention

- Set memory requests = limits to avoid overcommit-driven evictions
- Configure PodDisruptionBudget with `minAvailable: 1`
- Enable liveness and readiness probes on both gRPC and HTTP ports
- Set up a PagerDuty/Opsgenie integration for the `HebbsDown` alert
- Run HEBBS on dedicated node pools with resource guarantees

---

## Incident: Latency Degradation

**Alert:** `HebbsHighLatency`

### 1. Symptoms

- `remember` p99 latency exceeds 50ms sustained for 5+ minutes
- Other operations (recall, prime) may also show elevated latency
- Clients report timeouts or slow responses
- Dashboard "Latency" row shows upward trend

### 2. Immediate Triage

```bash
# Check current latency percentiles
curl -s http://localhost:6381/v1/metrics | grep hebbs_operation_duration_seconds

# Check for active RocksDB compactions
curl -s http://localhost:6381/v1/metrics | grep rocksdb_compaction

# Check CPU and memory utilization
kubectl top pod -l app=hebbs

# Check disk I/O (if node_exporter is available)
# node_disk_io_time_seconds_total for the HEBBS node

# Check for garbage collection pauses in logs
kubectl logs -l app=hebbs --tail=100 | grep -i "compaction\|stall\|slow"
```

### 3. Root Cause Analysis

Common causes, ranked by likelihood:

1. **RocksDB compaction pressure** — Heavy write load causes compaction to fall behind, increasing read amplification. Check `hebbs_rocksdb_compaction_pending` gauge.
2. **HNSW index saturation** — The in-memory HNSW graph for similarity search grows with memory count. At scale, recall queries traverse more nodes. Check if `memory_count` has grown significantly.
3. **Resource contention** — Another pod on the same node is competing for CPU or I/O. Check `kubectl top node`.
4. **Slow disk** — If running on HDD or a degraded SSD, compaction and reads suffer. Check `iostat` or node_exporter disk metrics.
5. **Large memory payloads** — A burst of oversized memories (large text content) increases serialization time. Check average memory size in recent writes.
6. **Network latency** — If the client-to-server network path has degraded. Check with `grpcurl` from within the cluster vs. externally.

### 4. Resolution

**Compaction pressure:**
```bash
# Increase background compaction threads
kubectl set env deployment/hebbs HEBBS_ROCKSDB_MAX_BACKGROUND_JOBS=4
kubectl rollout status deployment/hebbs

# If urgent, trigger a manual compaction (blocks writes briefly)
kubectl exec "$HEBBS_POD" -- hebbs db compact
```

**HNSW saturation:**
```bash
# Increase ef_search for better accuracy vs. speed trade-off,
# or decrease it to reduce traversal time at the cost of recall quality.
# This is a configuration change:
kubectl set env deployment/hebbs HEBBS_HNSW_EF_SEARCH=64
```

**Resource contention:**
```bash
# Move HEBBS to a dedicated node pool
kubectl patch deployment hebbs -p '{"spec":{"template":{"spec":{"nodeSelector":{"workload":"hebbs"}}}}}'
```

**Slow disk:**
```bash
# Migrate PVC to a faster storage class
# This requires creating a new PVC, restoring from backup, and redeploying.
```

### 5. Verification

```bash
# Watch p99 latency settle back below the threshold
watch -n 5 'curl -s http://localhost:6381/v1/metrics | grep "operation_duration_seconds" | grep "quantile=\"0.99\""'

# Run a targeted test
grpcurl -plaintext -d '{"content": "latency test memory"}' localhost:50051 hebbs.v1.HebbsService/Remember

# Confirm alert is no longer firing
# Check Prometheus Alerts page or Grafana alert list
```

### 6. Prevention

- Set `HEBBS_ROCKSDB_MAX_BACKGROUND_JOBS` to `min(cpu_count - 1, 4)` by default
- Size block cache appropriately (see Scaling Guide)
- Monitor `hebbs_rocksdb_compaction_pending` and alert before it becomes critical
- Use NVMe/SSD storage with guaranteed IOPS
- Set CPU requests to prevent throttling during compaction bursts
- Benchmark after every release to catch regressions

---

## Incident: Elevated Error Rate

**Alert:** `HebbsHighErrorRate`

### 1. Symptoms

- Error rate exceeds 5% of total requests for 5+ minutes
- Clients receive error responses (gRPC status codes, HTTP 4xx/5xx)
- Dashboard "Errors & Auth" row shows spikes in the error graph
- Specific operations may be disproportionately affected

### 2. Immediate Triage

```bash
# Break down errors by operation and type
curl -s http://localhost:6381/v1/metrics | grep hebbs_errors_total

# Check gRPC error status breakdown
curl -s http://localhost:6381/v1/metrics | grep hebbs_grpc_requests_total

# Check HTTP error status breakdown
curl -s http://localhost:6381/v1/metrics | grep hebbs_http_requests_total

# Check recent logs for error details
kubectl logs -l app=hebbs --tail=300 | grep -i "error\|ERR\|WARN"

# Check if a specific operation is failing
curl -s http://localhost:6381/v1/metrics | grep 'operation_duration_seconds_count.*status="error"'
```

### 3. Root Cause Analysis

Common causes, ranked by likelihood:

1. **Client-side errors (4xx)** — Invalid requests (malformed memories, missing required fields, exceeded size limits). Check `error_type` breakdown for `validation_error`.
2. **Embedding provider failure** — If HEBBS calls an external embedding API, it may be down or rate-limited. Check `error_type` for `embedding_error`.
3. **Disk errors** — RocksDB write failures due to disk full or I/O errors. Check `error_type` for `storage_error`.
4. **Authentication failures** — Misconfigured API keys or expired tokens. Check `error_type` for `auth_failure`. See [Incident: Authentication Failure Spike](#incident-authentication-failure-spike).
5. **Resource exhaustion** — Too many concurrent requests overwhelm the server. Check for `resource_exhausted` gRPC status codes.
6. **Bug in new release** — A regression introduced in a recent deploy. Check if the error spike correlates with a deployment event.

### 4. Resolution

**Client-side errors:**
```bash
# Identify the offending clients by checking access logs or adding client_id labels
kubectl logs -l app=hebbs --tail=200 | grep "validation_error"
# Coordinate with client team to fix request format
```

**Embedding provider failure:**
```bash
# Check the embedding provider status page
# Verify connectivity from the pod
kubectl exec "$HEBBS_POD" -- wget -qO- --timeout=5 https://api.openai.com/v1/models

# If the provider is down, HEBBS will queue and retry.
# Monitor the retry backlog:
curl -s http://localhost:6381/v1/metrics | grep embed
```

**Disk errors:**
```bash
# See Incident: Disk Capacity
```

**Recent deploy regression:**
```bash
# Check when the error rate spiked vs. last deploy time
kubectl rollout history deployment/hebbs

# Rollback if the spike correlates
kubectl rollout undo deployment/hebbs
kubectl rollout status deployment/hebbs
```

### 5. Verification

```bash
# Watch error rate drop below 5%
watch -n 10 'curl -s http://localhost:6381/v1/metrics | grep hebbs_errors_total'

# Run a smoke test across all operations
grpcurl -plaintext localhost:50051 hebbs.v1.HebbsService/Health

# Confirm the alert resolves
```

### 6. Prevention

- Validate request payloads at the client SDK level before sending to HEBBS
- Set up circuit breakers for the embedding provider with fallback behavior
- Monitor disk usage proactively with the `HebbsDiskAlmostFull` alert
- Canary deploys: route a fraction of traffic to the new version first
- Load test new releases with realistic traffic before production rollout

---

## Incident: Disk Capacity

**Alerts:** `HebbsDiskAlmostFull` (>85%), `HebbsDiskFull` (>95%)

### 1. Symptoms

- Disk usage ratio exceeds warning (85%) or critical (95%) thresholds
- Write operations start failing with `storage_error` at very high utilization
- RocksDB compaction stalls because there is no space for temporary files
- Pod may enter CrashLoopBackOff if RocksDB cannot open the database

### 2. Immediate Triage

```bash
# Check current disk usage
kubectl exec "$HEBBS_POD" -- df -h /data

# Check RocksDB disk usage breakdown
kubectl exec "$HEBBS_POD" -- du -sh /data/db/*

# Check how many memories are stored
curl -s http://localhost:6381/v1/metrics | grep hebbs_memory_count

# Check for compaction backlog (compaction produces temporary files)
curl -s http://localhost:6381/v1/metrics | grep rocksdb_compaction

# Check PVC capacity and usage
kubectl get pvc -l app=hebbs -o wide
```

### 3. Root Cause Analysis

Common causes, ranked by likelihood:

1. **Organic growth** — Memory count has grown beyond what the disk was sized for. Check `hebbs_memory_count` trend over the past week.
2. **Write amplification spike** — Heavy write/delete workloads cause RocksDB to amplify disk usage during compaction. Usually resolves after compaction completes.
3. **Stale memories** — Old, unused memories that could be cleaned up with `forget` operations. Check memory age distribution if available.
4. **WAL accumulation** — Write-ahead log files not being cleaned up, possibly due to a hung compaction. Check `/data/db/*.log` file sizes.
5. **Backup accumulation** — Old backup checkpoints stored on the same volume. Check `/data/backups/` size.

### 4. Resolution

**Immediate (>95% — critical):**
```bash
# 1. Remove old backups from the data volume
kubectl exec "$HEBBS_POD" -- rm -rf /data/backups/checkpoint-*

# 2. If that's not enough, trigger compaction to reclaim space from deleted keys
kubectl exec "$HEBBS_POD" -- hebbs db compact

# 3. If still critical, run an emergency forget sweep (remove oldest memories)
# WARNING: This deletes data. Ensure you have a backup first.
kubectl exec "$HEBBS_POD" -- hebbs forget --older-than 90d --dry-run
kubectl exec "$HEBBS_POD" -- hebbs forget --older-than 90d --confirm
```

**Planned (>85% — warning):**
```bash
# 1. Resize the PVC (if your storage class supports expansion)
kubectl patch pvc hebbs-data -p '{"spec":{"resources":{"requests":{"storage":"100Gi"}}}}'

# 2. If PVC expansion is not supported, migrate to a larger volume:
#    a. Create backup
#    b. Create new, larger PVC
#    c. Restore to new PVC
#    d. Update deployment to use new PVC

# 3. Implement a retention policy via scheduled forget jobs
```

### 5. Verification

```bash
# Confirm disk usage has dropped
kubectl exec "$HEBBS_POD" -- df -h /data

# Confirm writes are succeeding
grpcurl -plaintext -d '{"content": "disk check memory"}' localhost:50051 hebbs.v1.HebbsService/Remember

# Confirm the alert resolves in Prometheus
```

### 6. Prevention

- Size disks with the formula in the Scaling Guide (2.5x multiplier)
- Set up the `HebbsDiskAlmostFull` alert at 85% to get early warning
- Store backups on a separate volume or object storage, not the data PVC
- Implement automated retention policies for old memories
- Monitor disk growth rate and project when expansion is needed

---

## Incident: Reflect Pipeline Stall

**Alert:** `HebbsReflectStalled`

### 1. Symptoms

- No successful reflect runs in 48+ hours despite having stored memories
- `hebbs_reflect_runs_total` counter is flat
- Insights are stale or missing for recent memories
- Dashboard "Cognitive Health" row shows no activity

### 2. Immediate Triage

```bash
# Check last reflect run timestamp
curl -s http://localhost:6381/v1/metrics | grep hebbs_reflect

# Check if reflect is enabled in configuration
kubectl get deployment hebbs -o jsonpath='{.spec.template.spec.containers[0].env}' | jq '.[] | select(.name | startswith("HEBBS_REFLECT"))'

# Check for reflect-specific errors in logs
kubectl logs -l app=hebbs --tail=500 | grep -i "reflect"

# Check LLM provider connectivity
kubectl exec "$HEBBS_POD" -- wget -qO- --timeout=10 https://api.openai.com/v1/models 2>&1 | head -5

# Check memory count (reflect needs memories to process)
curl -s http://localhost:6381/v1/metrics | grep hebbs_memory_count
```

### 3. Root Cause Analysis

Common causes, ranked by likelihood:

1. **LLM provider outage or rate limit** — The external LLM API used for reflection is unavailable or returning 429 errors. Check provider status page and pod logs.
2. **Configuration error** — Reflect is disabled, the API key is expired, or the endpoint is misconfigured. Check environment variables.
3. **Reflect scheduler stuck** — An internal scheduling issue prevents reflect from triggering. Check for stuck threads or deadlocks in logs.
4. **No new memories to process** — Reflect may only run when new memories accumulate past a threshold. Check if `hebbs_memory_count` has been static.
5. **Resource starvation** — Reflect runs on background threads. If CPU is fully consumed by read/write operations, reflect may be starved.

### 4. Resolution

**LLM provider issue:**
```bash
# Verify the API key
kubectl get secret hebbs-secrets -o jsonpath='{.data.LLM_API_KEY}' | base64 -d | head -c 10
echo "..."

# Test the LLM endpoint directly
kubectl exec "$HEBBS_POD" -- wget -qO- --timeout=10 \
  --header="Authorization: Bearer $LLM_API_KEY" \
  https://api.openai.com/v1/models

# If the provider is down, wait for recovery. HEBBS will retry automatically.
# If the key is expired, rotate it:
kubectl create secret generic hebbs-secrets --from-literal=LLM_API_KEY=new-key-here --dry-run=client -o yaml | kubectl apply -f -
kubectl rollout restart deployment/hebbs
```

**Configuration error:**
```bash
# Ensure reflect is enabled
kubectl set env deployment/hebbs HEBBS_REFLECT_ENABLED=true
kubectl set env deployment/hebbs HEBBS_REFLECT_INTERVAL_SECS=3600
kubectl rollout status deployment/hebbs
```

**Trigger a manual reflect run:**
```bash
kubectl exec "$HEBBS_POD" -- hebbs reflect --run-now
```

### 5. Verification

```bash
# Watch for a new reflect run
watch -n 30 'curl -s http://localhost:6381/v1/metrics | grep hebbs_reflect_runs_total'

# Check logs for successful completion
kubectl logs -l app=hebbs --tail=50 | grep -i "reflect.*complete\|reflect.*success"

# Verify insights are being generated
grpcurl -plaintext -d '{"limit": 5}' localhost:50051 hebbs.v1.HebbsService/Insights
```

### 6. Prevention

- Monitor the `HebbsReflectStalled` alert — 48 hours is the early warning
- Set up a separate alert for LLM provider error rates
- Configure reflect with a dead-letter mechanism for failed runs
- Ensure adequate CPU headroom for background reflect processing
- Log LLM API response codes and latency for observability

---

## Incident: Authentication Failure Spike

**Alert:** `HebbsAuthFailureSpike`

### 1. Symptoms

- Authentication failure rate exceeds 10% of total requests
- Clients receive `Unauthenticated` gRPC status or HTTP 401
- `hebbs_errors_total{error_type="auth_failure"}` counter is climbing rapidly
- Dashboard "Errors & Auth" row shows auth_failure spike

### 2. Immediate Triage

```bash
# Check auth failure rate
curl -s http://localhost:6381/v1/metrics | grep 'error_type="auth_failure"'

# Check total request rate to understand the ratio
curl -s http://localhost:6381/v1/metrics | grep -E '(grpc_requests_total|http_requests_total)'

# Check logs for auth failure details (client IPs, methods)
kubectl logs -l app=hebbs --tail=300 | grep -i "auth\|unauthenticated\|401\|forbidden"

# Check if the auth secret/config is mounted correctly
kubectl get secret hebbs-secrets -o yaml | grep -c "data"
kubectl exec "$HEBBS_POD" -- ls -la /etc/hebbs/

# Check if a recent deploy changed auth configuration
kubectl rollout history deployment/hebbs
```

### 3. Root Cause Analysis

Common causes, ranked by likelihood:

1. **API key rotation without client update** — The server-side key was rotated but clients are still using the old key. Check if a key rotation happened recently.
2. **Client misconfiguration** — A new client or updated client SDK is sending incorrect credentials. Check which clients are failing by examining source IPs in logs.
3. **Expired tokens** — If using JWT/OAuth, tokens may have expired without refresh. Check token expiry timestamps.
4. **Brute-force attempt** — An unauthorized actor is attempting to guess credentials. Check for high-volume requests from unknown IPs.
5. **Secret mount failure** — The Kubernetes secret containing the auth config is not mounted or is empty. Check volume mounts.

### 4. Resolution

**Key rotation mismatch:**
```bash
# Check the current server-side key
kubectl get secret hebbs-secrets -o jsonpath='{.data.API_KEY}' | base64 -d | head -c 10
echo "..."

# If the old key needs to be temporarily re-added for client migration:
# Update the secret to accept both old and new keys (if HEBBS supports key lists)
kubectl create secret generic hebbs-secrets \
  --from-literal=API_KEY=new-key \
  --from-literal=API_KEY_PREVIOUS=old-key \
  --dry-run=client -o yaml | kubectl apply -f -
kubectl rollout restart deployment/hebbs

# Coordinate with client teams to update their API keys
```

**Brute-force attempt:**
```bash
# Identify the source IPs
kubectl logs -l app=hebbs --tail=1000 | grep "auth_failure" | awk '{print $NF}' | sort | uniq -c | sort -rn | head -10

# Block at the network policy level
kubectl apply -f - <<EOF
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: hebbs-block-abuser
spec:
  podSelector:
    matchLabels:
      app: hebbs
  ingress:
  - from:
    - ipBlock:
        cidr: 0.0.0.0/0
        except:
        - ABUSER_IP/32
EOF
```

**Secret mount failure:**
```bash
kubectl describe pod -l app=hebbs | grep -A 5 "Mounts\|Volumes"
# If the secret is not mounted, check the deployment spec and fix the volume mount
```

### 5. Verification

```bash
# Watch auth failure rate decrease
watch -n 10 'curl -s http://localhost:6381/v1/metrics | grep auth_failure'

# Test authentication with correct credentials
grpcurl -plaintext -H "Authorization: Bearer $API_KEY" localhost:50051 hebbs.v1.HebbsService/Health

# Confirm the alert resolves
```

### 6. Prevention

- Implement graceful key rotation: accept both old and new keys during a migration window
- Set key expiry alerts before keys expire
- Use short-lived tokens with automatic refresh where possible
- Rate limit authentication attempts per source IP
- Set up NetworkPolicies to restrict access to known client CIDRs
- Audit auth failure logs regularly for anomalous patterns

---

## Incident: OOM Kill

**Alert:** None (detected via pod restart with `OOMKilled` reason)

### 1. Symptoms

- Pod restarts with `lastState.terminated.reason: OOMKilled`
- `kubectl get pods` shows high restart count
- Brief service interruptions as the pod restarts
- May trigger `HebbsDown` alert during restart window
- RocksDB recovery runs on startup (may add 10–60 seconds to boot time)

### 2. Immediate Triage

```bash
# Confirm OOM kill
kubectl get pods -l app=hebbs -o jsonpath='{range .items[*]}{.metadata.name}{"\t"}{.status.containerStatuses[0].restartCount}{"\t"}{.status.containerStatuses[0].lastState.terminated.reason}{"\t"}{.status.containerStatuses[0].lastState.terminated.exitCode}{"\n"}{end}'

# Check current memory usage vs. limits
kubectl top pod -l app=hebbs
kubectl get deployment hebbs -o jsonpath='{.spec.template.spec.containers[0].resources}'

# Check memory count (correlates with memory usage)
curl -s http://localhost:6381/v1/metrics | grep hebbs_memory_count

# Check node memory pressure
kubectl describe node $(kubectl get pods -l app=hebbs -o jsonpath='{.items[0].spec.nodeName}') | grep -A 5 "Conditions"

# Check if there was a memory spike in Prometheus
# Query: container_memory_working_set_bytes{pod=~"hebbs.*"}
```

### 3. Root Cause Analysis

Common causes, ranked by likelihood:

1. **Undersized memory limits** — Memory count has grown beyond what the current limits support. Use the RAM sizing formula to calculate the correct limit.
2. **Block cache too large** — `HEBBS_BLOCK_CACHE_MB` is set higher than what the pod limit allows. The block cache + heap usage exceeds the container limit.
3. **Memory leak** — A bug causing unbounded memory growth. Check if memory usage grows monotonically even with stable memory count. Requires a memory profile.
4. **Burst of large memories** — A sudden influx of large-payload memories causes temporary memory spikes. Check recent write patterns.
5. **Node-level overcommit** — Other pods on the same node are consuming excess memory, and the OOM killer selects HEBBS. Check `kubectl top node` and pod QoS class.

### 4. Resolution

**Undersized limits (most common):**
```bash
# Calculate the correct limit using the sizing formula
# RAM_MB = block_cache_mb + (memory_count × 0.00005) + 200

# Check current values
MEMORY_COUNT=$(curl -s http://localhost:6381/v1/metrics | grep hebbs_memory_count | awk '{print $2}')
BLOCK_CACHE=$(kubectl get deployment hebbs -o jsonpath='{.spec.template.spec.containers[0].env}' | jq -r '.[] | select(.name=="HEBBS_BLOCK_CACHE_MB") | .value // "256"')

echo "Memory count: $MEMORY_COUNT"
echo "Block cache: ${BLOCK_CACHE}MB"
echo "Calculated RAM: $(echo "$BLOCK_CACHE + $MEMORY_COUNT * 0.00005 + 200" | bc)MB"

# Increase the limit (add 20% headroom above calculated)
kubectl patch deployment hebbs -p '{
  "spec": {
    "template": {
      "spec": {
        "containers": [{
          "name": "hebbs",
          "resources": {
            "requests": {"memory": "NEW_VALUEMi"},
            "limits": {"memory": "NEW_VALUEMi"}
          }
        }]
      }
    }
  }
}'
kubectl rollout status deployment/hebbs
```

**Block cache too large:**
```bash
# Reduce block cache to fit within the pod memory limit
kubectl set env deployment/hebbs HEBBS_BLOCK_CACHE_MB=256
kubectl rollout status deployment/hebbs
```

**Node overcommit:**
```bash
# Set QoS to Guaranteed by making requests = limits
# This ensures the OOM killer targets BestEffort/Burstable pods first
kubectl patch deployment hebbs -p '{
  "spec": {
    "template": {
      "spec": {
        "containers": [{
          "name": "hebbs",
          "resources": {
            "requests": {"memory": "2Gi", "cpu": "2"},
            "limits": {"memory": "2Gi", "cpu": "2"}
          }
        }]
      }
    }
  }
}'
```

### 5. Verification

```bash
# Confirm the pod is stable (no restarts)
kubectl get pods -l app=hebbs -w

# After 10 minutes, check memory usage is within limits
kubectl top pod -l app=hebbs

# Confirm healthy operation
curl -s http://localhost:6381/v1/health | jq .
grpcurl -plaintext localhost:50051 hebbs.v1.HebbsService/Health
```

### 6. Prevention

- Use the RAM sizing formula proactively as memory count grows
- Set `requests` = `limits` for Guaranteed QoS class
- Monitor `container_memory_working_set_bytes` and alert at 80% of the limit
- Add a custom alert: `container_memory_working_set_bytes{pod=~"hebbs.*"} / container_spec_memory_limit_bytes > 0.85`
- Set `HEBBS_BLOCK_CACHE_MB` explicitly rather than relying on defaults
- Run on dedicated nodes to avoid competing for memory with noisy neighbors
- Profile memory usage after major feature releases

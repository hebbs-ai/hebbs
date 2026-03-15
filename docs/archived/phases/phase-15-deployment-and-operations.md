# Phase 15: Deployment and Operations -- Architecture Blueprint

## Status: COMPLETE

---

## Intent

Phases 1 through 14 built HEBBS: engine, server, SDKs, benchmarks, hardening, and a reference application that proves the system works. But all of that runs on a developer's laptop. A `cargo build --release` and a `python -m hebbs_demo interactive` on a MacBook is not a production system. Phase 15 exists to close the gap between "it works on my machine" and "it is running in production with observability, alerting, and a documented recovery path."

The gap is operational maturity. Today, deploying HEBBS requires a developer who understands Rust toolchains, RocksDB data directories, ONNX model paths, gRPC port configuration, and Prometheus scrape endpoints. An operations engineer at a customer site should not need to know any of that. They should pull a container image, apply a Helm chart, and have a working HEBBS instance within minutes. When something breaks at 3 AM, they should open a runbook, not source code.

Phase 15 serves four audiences:

- **Platform engineers at customer sites:** They need Docker images that follow container best practices (non-root, health checks, signal handling, minimal attack surface), Helm charts that integrate with their existing Kubernetes infrastructure (StorageClasses, Ingress controllers, cert-manager, Prometheus Operator), and Terraform modules that provision the full stack in their cloud account. If any of these require custom scripting or manual steps beyond `helm install`, the deployment model is broken.

- **The HEBBS operations team:** Before handing deployment artifacts to customers, the team must run HEBBS in production themselves. This means runbooks that codify the operational knowledge accumulated across 14 phases. Undocumented tribal knowledge is a single-point-of-failure that does not survive team changes.

- **Prospective customers evaluating HEBBS:** `docker run hebbs-ai/hebbs` followed by a `curl` to the health endpoint is the fastest proof-of-life. If the evaluation requires compiling Rust, the trial ends before it begins. The Docker image is the front door.

- **The CI/CD pipeline:** Phase 12 built binary release pipelines for three targets (linux-x86_64, linux-aarch64, macos-arm64). Phase 15 extends this to produce container images on every release tag, push them to a registry, and gate releases on container-level smoke tests. The container image becomes a first-class release artifact alongside the tarball.

**Distribution philosophy:** HEBBS follows the same pattern as Redis, Qdrant, and every other serious infrastructure product. The product ships as a server image with a `/metrics` endpoint. Monitoring integration (Prometheus scraping, Grafana dashboards, alerting rule import) is documentation and tutorials, not bundled product artifacts. Phase 15 produces the Grafana dashboard JSON and Prometheus alerting rules as standalone files. Phase 16 (Documentation Site) publishes the tutorials explaining how to wire them into a customer's monitoring stack -- just as Redis publishes "Monitor Redis with Prometheus and Grafana" as a docs tutorial, not as a Docker Compose in the product repo.

The decisions made here -- base image, volume layout, resource defaults -- will be inherited by every customer deployment. Getting them right means fewer support tickets. Getting them wrong means debugging production environments where the HEBBS team has no access.

---

## Scope Boundaries

### What Phase 15 delivers

- Multi-stage Dockerfile producing a minimal, secure container image for `hebbs-server`
- Helm chart for Kubernetes deployment (StatefulSet, PersistentVolumeClaim, ConfigMap, Secret, Service, Ingress, ServiceMonitor)
- At least one Terraform module (AWS EKS + EBS + ALB) that provisions a production-ready HEBBS deployment from scratch
- Grafana dashboard JSON published as a standalone file (not wired into a running Grafana instance)
- Prometheus alerting rules published as a standalone file (not wired into a running Prometheus instance)
- Operations runbook: backup, restore, scaling, incident response, upgrade procedures
- CI/CD extension: container image build and push on release tags, container smoke test in CI
- Example `hebbs.toml` configuration file for production, annotated with commentary

### What Phase 15 explicitly does NOT deliver

- Horizontal scaling or sharding (HEBBS is single-node; Phase 17 introduces sync between instances, but Phase 15 deploys one instance per StatefulSet replica)
- Custom Kubernetes operator (a Helm chart is sufficient for the current complexity; an operator is warranted only when lifecycle management exceeds what Helm can express)
- A managed SaaS control plane (this is infrastructure-as-code for self-hosted deployment, not a hosted service)
- Windows container images (Linux and macOS only; Windows deployment is not a target audience)
- CDN or edge deployment (Phase 17 covers edge mode)
- Load testing infrastructure (Phase 12's `hebbs-bench` already exists; Phase 15 makes it runnable against a deployed instance, but does not build new load testing tools)
- Automated failover or replication (single-node; HA is a post-Phase-17 concern requiring sync)
- Docker Compose with Prometheus + Grafana (this is a documentation tutorial, not a product artifact -- delivered in Phase 16)
- "How to monitor HEBBS" tutorials (Phase 16 -- follows the Redis model where monitoring setup guides are documentation, not shipped product)
- Grafana dashboard walkthrough or alerting rules import guide (Phase 16)

---

## Architectural Decisions

### 1. Repository Layout: Monorepo Dockerfile, Separate Deploy Repo

`RepositoryStructure.md` specifies two locations for deployment artifacts:
- `hebbs/docker/` in the core monorepo for the Dockerfile and Docker Compose
- `hebbs-deploy` as a separate repository for Helm charts, Terraform modules, monitoring dashboards, and runbooks

**Why this split:**

The Dockerfile changes when the binary changes -- new dependencies, new ports, new health check paths, new configuration flags. It must be tested in the same CI pipeline that builds the binary. Co-locating it in the monorepo makes this trivial: the release workflow builds the binary and the container image in the same pipeline, from the same commit.

The Helm chart, Terraform modules, and Grafana dashboards change on a different cadence. A customer may upgrade their Helm chart values (resource limits, replica count, storage class) without upgrading the HEBBS version. Or they may upgrade HEBBS without changing their infrastructure. Independent release cadences require independent repositories.

**Concrete layout:**

In `hebbs/` (core monorepo):
- `docker/Dockerfile` -- the multi-stage build

In `hebbs-deploy/` (new repository):
- `helm/hebbs/` -- the Helm chart
- `terraform/aws/` -- AWS EKS module
- `terraform/gcp/` -- GCP GKE module (placeholder, delivered if time allows)
- `dashboards/` -- Grafana dashboard JSON (standalone files, importable by customers into their existing Grafana)
- `alerts/` -- Prometheus alerting rules (standalone files, importable into existing Prometheus)
- `runbooks/` -- Operational runbooks in Markdown
- `examples/` -- Example `hebbs.toml` configurations for common deployment patterns

### 2. Docker Image Strategy: Multi-Stage Build on Distroless

The container image must satisfy two competing constraints: minimal attack surface (Principle 12: Secure by Default) and minimal size for fast pull times during scaling events.

**Build stage:** Use the official `rust:1.75-bookworm` image (or the `rust-version` from workspace `Cargo.toml`). Compile with the release profile (`lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`) that is already defined in the workspace `Cargo.toml`. Build targets: `hebbs-server`, `hebbs-cli`, `hebbs-bench`. All three binaries are useful in the container -- the server runs, the CLI is for debugging (`kubectl exec` into the pod and run `hebbs-cli status`), and the bench binary enables in-situ performance validation.

**Runtime stage:** Use `gcr.io/distroless/cc-debian12` as the base. Distroless contains no shell, no package manager, no unnecessary libraries. The HEBBS binary is statically linked against RocksDB (via the `rocksdb` crate's bundled build) and dynamically linked only against glibc and libgcc -- both present in the distroless `cc` variant.

**Why distroless over scratch:** Scratch contains literally nothing -- no CA certificates, no timezone data, no glibc. HEBBS needs:
- CA certificates for ONNX model download on first run (HTTPS to Hugging Face)
- CA certificates for LLM provider API calls (reflect pipeline reaching Anthropic/OpenAI/Ollama endpoints)
- Timezone data for log timestamps
- glibc for RocksDB's C++ runtime

Distroless `cc` provides all of these at ~20MB. Alpine with musl is an alternative but RocksDB has known compatibility issues with musl's allocator under high write load. glibc is the safe choice.

**Image sizing target:** The final image should be under 80MB. Breakdown: ~20MB distroless base + ~40MB HEBBS binaries (server + cli + bench, stripped) + ~5MB miscellaneous. The ONNX model (~33MB) is NOT baked into the image -- it is downloaded on first run or mounted as a volume. Baking it in would bloat every image pull by 33MB and prevent model upgrades without image rebuilds.

**Non-root execution:** The container runs as a non-root user (UID 65532, the distroless `nonroot` user). The data directory and model cache directory must be writable by this user. The Dockerfile creates these directories with correct ownership.

**Signal handling:** HEBBS already handles SIGTERM for graceful shutdown (Phase 8). The Dockerfile sets `STOPSIGNAL SIGTERM`. Kubernetes sends SIGTERM on pod termination; the `terminationGracePeriodSeconds` in the Helm chart must exceed the server's `shutdown_timeout_secs` (default 5s) by a comfortable margin.

**Health checks at the Docker layer:** The Dockerfile includes a `HEALTHCHECK` instruction using `hebbs-cli status --endpoint localhost:6380 --timeout 2s`. This provides Docker-native health visibility (via `docker ps` and `docker inspect`) independently of Kubernetes probes. The health check calls the gRPC health service that Phase 8 already implements.

### 3. Helm Chart: StatefulSet, Not Deployment

HEBBS stores data on disk (RocksDB). Pods with local state require StatefulSets, not Deployments. The distinction matters:

- StatefulSet provides stable pod identity (`hebbs-0`, `hebbs-1`, ...) and stable persistent volume bindings. When `hebbs-0` is rescheduled to a different node, it re-attaches the same PersistentVolume.
- Deployment provides no storage guarantee. Rescheduling creates a new pod with a new empty volume. For HEBBS, this means data loss.

**Replicas and the scaling model:**

Phase 15 deploys HEBBS as a single-replica StatefulSet. The Helm chart accepts `replicaCount` for future use, but the documentation clearly states that multiple replicas are independent instances with independent data -- there is no replication or consensus. This is accurate and honest. Horizontal scaling with shared state requires the sync protocol (Phase 17). Setting `replicaCount > 1` in Phase 15 gives you N independent HEBBS instances behind a load balancer, each with its own memory store. This is useful for multi-tenant deployments where each tenant gets a dedicated instance, but it is not horizontal scaling of a shared dataset.

**PersistentVolumeClaim:**

Each pod gets a PVC from the configured StorageClass. The default storage request is 20Gi (sufficient for ~20M memories at ~1KB per memory). The StorageClass is configurable because cloud providers use different provisioners (gp3 on AWS, pd-ssd on GCP, managed-premium on Azure). The access mode is `ReadWriteOnce` -- a single pod owns the volume exclusively. RocksDB does not support concurrent access from multiple processes.

Performance characteristics of the storage class matter enormously. RocksDB's write path (WAL + memtable flush + compaction) is I/O-bound. The Helm chart values document minimum IOPS recommendations: 3,000 IOPS baseline for production workloads, 500 IOPS for evaluation. AWS gp3 provides 3,000 baseline IOPS by default. GCP pd-ssd scales IOPS with volume size (30 IOPS/GiB, so 20Gi gives 600 -- production workloads should use 100Gi+). This nuance is documented in the chart's `values.yaml` comments and the runbook.

**ConfigMap and Secret:**

The Helm chart separates configuration into two Kubernetes resources:
- `ConfigMap`: `hebbs.toml` contents (server ports, storage paths, embedding config, decay settings, logging level, metrics endpoint). No sensitive data.
- `Secret`: API keys (for reflect LLM providers), auth keys file content, TLS certificates. Referenced via environment variables in the pod spec, never mounted as files in a world-readable location.

Configuration precedence (already implemented in Phase 8): CLI flags > environment variables > config file > compiled defaults. The Helm chart uses environment variables for Secret-sourced values and a mounted ConfigMap for the TOML file. This means sensitive values never appear in the ConfigMap and are not logged by `hebbs-server config-dump`.

**Health probes:**

Three Kubernetes probes, each mapped to existing Phase 8 endpoints:

| Probe | Endpoint | Protocol | Failure Behavior |
|-------|----------|----------|------------------|
| Startup | `/v1/health/ready` (HTTP) | HTTP GET | Blocks traffic until engine is initialized (HNSW rebuilt, decay started) |
| Liveness | gRPC `grpc.health.v1.Health/Check` | gRPC | Pod restart on consecutive failures (indicates hung process) |
| Readiness | `/v1/health/ready` (HTTP) | HTTP GET | Removes pod from Service endpoints (stops routing traffic during compaction storms or shutdown) |

Why different protocols for liveness vs readiness: The liveness probe uses gRPC because it tests the gRPC server loop itself. If the gRPC server is unresponsive, the process is hung and must be restarted. The readiness probe uses HTTP because it tests overall system health including RocksDB availability and index initialization -- conditions where the process is alive but not ready to serve requests.

**Startup probe rationale:** HEBBS rebuilds its in-memory HNSW graph from the `vectors` column family on startup. At 1M memories, this rebuild takes 10-30 seconds. Without a startup probe, the liveness probe would kill the pod during reconstruction. The startup probe gives the engine time to initialize (configurable `initialDelaySeconds`, default 30s, `failureThreshold` of 30 with 10s period = 5 minutes maximum startup time).

**Service and Ingress:**

The Helm chart creates a Kubernetes Service with two named ports: `grpc` (6380) and `http` (6381). Both are `ClusterIP` by default (internal-only). External access is via an Ingress resource (disabled by default, enabled via `ingress.enabled=true`).

gRPC through Kubernetes Ingress requires HTTP/2. This means:
- NGINX Ingress Controller needs `nginx.ingress.kubernetes.io/backend-protocol: "GRPC"` annotation.
- AWS ALB Ingress Controller natively supports gRPC via target group protocol version.
- The Helm chart provides annotation presets for the most common Ingress controllers, selectable via `ingress.className`.

The HTTP REST endpoint works through any standard Ingress without special configuration.

**ServiceMonitor (Prometheus Operator integration):**

If the target cluster runs the Prometheus Operator (kube-prometheus-stack), the Helm chart creates a `ServiceMonitor` resource that automatically configures Prometheus to scrape HEBBS's metrics endpoint. This is gated behind `metrics.serviceMonitor.enabled` (default true when `metrics.enabled` is true). The ServiceMonitor specifies:
- Scrape path: `/v1/metrics`
- Scrape port: `http` (6381)
- Scrape interval: 15 seconds
- Metric relabeling: adds `hebbs_instance` label from pod name

**Resource requests and limits:**

The Helm chart sets default resource requests and limits based on the benchmark data from Phase 12:

| Resource | Request | Limit | Rationale |
|----------|---------|-------|-----------|
| CPU | 500m | 2000m | Phase 12 benchmarks show HEBBS saturates ~1 core under full load. 2 cores allows for compaction. |
| Memory | 512Mi | 2Gi | 256MB RocksDB block cache (default) + HNSW in-memory index (~50 bytes per node × N memories) + ONNX runtime. At 1M memories, HNSW index is ~50MB. The 2Gi limit provides ample headroom. |

These are defaults. The values file documents the formula for calculating memory requirements at different scales: `RAM_MB ≈ block_cache_mb + (memory_count × 0.00005) + 200 (base overhead)`.

**Pod disruption budget:**

For `replicaCount >= 2`, the chart creates a PodDisruptionBudget ensuring at least one pod remains available during voluntary disruptions (node drains, cluster upgrades). For `replicaCount = 1`, no PDB is created (a single pod cannot tolerate any disruption).

### 4. Terraform: AWS EKS Module as the Reference

One fully working Terraform module for AWS. A GCP module as a stretch goal (documented as placeholder structure if not completed).

**Why AWS first:** AWS has the largest market share among the target customer base (YC-backed AI companies). EKS is the most common Kubernetes platform. gp3 EBS volumes provide the IOPS profile HEBBS needs at reasonable cost. ALB supports gRPC natively.

**Module scope:**

The Terraform module provisions the complete stack from a blank AWS account (with appropriate IAM permissions):

1. **VPC:** Private subnets for EKS nodes, public subnets for the ALB. NAT gateway for outbound (model downloads, LLM API calls). The module optionally uses an existing VPC (via `vpc_id` variable) to avoid creating redundant networking in accounts that already have infrastructure.

2. **EKS Cluster:** Managed node group with configurable instance types. Default: `m6i.large` (2 vCPU, 8GB RAM) -- sufficient for a single HEBBS instance serving up to 10M memories. The module uses EKS managed add-ons for CoreDNS, kube-proxy, and the VPC CNI.

3. **EBS CSI Driver:** Required for dynamic PV provisioning on EKS. The module installs the EBS CSI driver add-on and creates an IAM role for the service account (IRSA). The StorageClass is created with `gp3` volume type, 3000 IOPS, and 125 MB/s throughput.

4. **ALB Controller:** For Ingress. The module installs the AWS Load Balancer Controller and configures IAM permissions. The Helm chart's Ingress resource uses ALB annotations.

5. **Helm Release:** The module runs `helm install hebbs hebbs-deploy/helm/hebbs` with values derived from Terraform variables (storage class name, node selectors, ALB annotations, Secret values from AWS Secrets Manager).

**What the module does NOT provision:**
- DNS records (customer's domain, customer's Route53 zone)
- TLS certificates (customer's cert-manager or ACM setup)
- The Helm chart content itself (that comes from `hebbs-deploy` repo, the Terraform module references it)

### 5. Monitoring Artifacts: Dashboard JSON and Alerting Rules

Phase 15 produces the Grafana dashboard JSON and Prometheus alerting rules as standalone files that customers import into their existing monitoring infrastructure. Phase 16 publishes the tutorials explaining how to set up Prometheus scraping, import the dashboard, and configure alerting -- following the Redis model where monitoring setup is documentation, not bundled product.

The Grafana dashboard is not a wall of numbers. It is organized into panels that answer operational questions in priority order.

**Row 1: "Is it working?"** (the 3 AM glance)
- Service health status (green/yellow/red derived from readiness probe)
- Request rate (ops/sec, stacked by operation type)
- Error rate (errors/sec, stacked by error category)
- p99 latency (single stat, current value vs SLO threshold line)

**Row 2: "How fast is it?"** (latency analysis)
- `remember` latency histogram (p50, p95, p99) over time
- `recall` latency histogram by strategy (similarity, temporal, causal, analogical) over time
- `subscribe` push latency histogram over time
- `prime` latency histogram over time

**Row 3: "How full is it?"** (capacity planning)
- Memory count (gauge, current value + trend line)
- Disk usage bytes (gauge, current + trend, with configurable threshold line at 80% capacity)
- HNSW index size (node count, memory estimate)
- Active subscriptions (gauge)
- RocksDB compaction stats (bytes written/read, stall duration)

**Row 4: "Is it learning?"** (cognitive health)
- Reflect cycle count and last reflect timestamp
- Insights created per reflect cycle
- Decay sweep progress (cursor position, memories scored per sweep)
- Auto-forget candidates identified

**Row 5: "Is it secure?"** (auth and tenancy)
- Authentication failures (counter, spike = possible attack)
- Requests by tenant (stacked area, verify isolation)
- Rate limit rejections (counter, spike = configuration too tight or abuse)

**Dashboard variables (Grafana template variables):**
- `instance` (filter by HEBBS instance when running multiple)
- `tenant` (filter by tenant ID for multi-tenant deployments)
- `interval` (auto or manual scrape interval alignment)

**Alerting rules (Prometheus):**

| Alert | Condition | Severity | Runbook Section |
|-------|-----------|----------|-----------------|
| `HebbsDown` | `up{job="hebbs"} == 0` for 2 minutes | Critical | "Instance Unreachable" |
| `HebbsHighLatency` | `histogram_quantile(0.99, remember_latency) > 50ms` for 5 minutes | Warning | "Latency Degradation" |
| `HebbsHighErrorRate` | `rate(errors_total[5m]) > 0.05 * rate(requests_total[5m])` (>5% error rate) | Warning | "Elevated Error Rate" |
| `HebbsDiskAlmostFull` | `disk_usage_bytes / disk_capacity_bytes > 0.85` | Warning | "Disk Capacity" |
| `HebbsDiskFull` | `disk_usage_bytes / disk_capacity_bytes > 0.95` | Critical | "Disk Capacity -- Immediate" |
| `HebbsReflectStalled` | No `reflect_cycle_total` increase in 48 hours when `reflect_enabled == true` | Warning | "Reflect Pipeline Stall" |
| `HebbsCompactionStall` | `rocksdb_stall_duration_seconds > 0` for 10 minutes | Warning | "Compaction Stall" |
| `HebbsAuthFailureSpike` | `rate(auth_failures_total[5m]) > 10` | Warning | "Authentication Failure Spike" |

Each alert's `annotations.runbook_url` points to the corresponding runbook section. On-call engineers follow the link, not the alert message.

### 6. Runbook: The On-Call Engineer's Bible

The runbook is a Markdown document organized by failure scenario, not by system component. Each section follows the same template:

**Template:**
1. **Symptoms** -- What the alert says, what the dashboard shows, what the user reports.
2. **Immediate triage** -- Commands to run in the first 60 seconds to determine scope and severity.
3. **Root cause analysis** -- Common causes ranked by likelihood, with diagnostic commands for each.
4. **Resolution** -- Step-by-step fix for each root cause.
5. **Verification** -- How to confirm the fix worked.
6. **Prevention** -- Configuration or code changes to prevent recurrence.

**Runbook sections:**

- **Backup and Restore:** RocksDB checkpoint-based backup procedure. `hebbs-server` exposes no backup API today, so the procedure uses RocksDB's filesystem-level checkpoint (create a hard-link snapshot of the data directory while the server is running -- RocksDB guarantees consistency of checkpoints). Restore: stop server, replace data directory with checkpoint, start server. The HNSW in-memory index rebuilds from the `vectors` column family automatically. Estimated restore time: ~30 seconds per 1M memories (dominated by HNSW rebuild).

- **Scaling Guide:** Vertical scaling only in Phase 15. The critical resources are: (a) RAM -- increase for larger block cache and HNSW index, formula provided; (b) IOPS -- increase for higher write throughput, measure with `rocksdb_write_stall` metric; (c) CPU -- increase for concurrent recall under load. The guide provides a sizing table mapping memory count to recommended instance type.

- **Upgrade Procedure:** Rolling upgrade is not supported for single-replica (stop, pull new image, start). For multi-replica independent instances: drain one pod at a time, upgrade, verify health, proceed. The guide documents which version transitions require data migration (none so far, but the process for future migrations is documented).

- **Incident Response:** Latency spike, disk full, OOM kill, HNSW corruption (detected via recall returning zero results for known-good queries), reflect pipeline failure (LLM provider unreachable), authentication bypass attempt.

### 7. CI/CD Extension: Container Image as Release Artifact

The existing `release.yml` workflow builds binaries and creates GitHub Releases on `v*` tags. Phase 15 extends this pipeline with container image build and push.

**Container registry:** GitHub Container Registry (`ghcr.io/hebbs-ai/hebbs`). No external registry dependency. The image is public for evaluation, gated behind GitHub auth for production pulls if needed.

**Workflow extension:**

A new job in `release.yml` (or a new `docker.yml` workflow triggered by the same `v*` tags):

1. Build the container image using the multi-stage Dockerfile. Multi-platform build via `docker buildx` for `linux/amd64` and `linux/arm64`.
2. Tag the image with the version (`v0.1.0`), `latest`, and the Git SHA.
3. Push to `ghcr.io`.
4. Run a container smoke test: start the container, wait for health check, run `hebbs-cli remember "smoke test"` and `hebbs-cli recall "smoke test"` against the containerized server, verify non-empty recall result, stop the container.

**Why multi-platform:** The aarch64 image serves ARM-based cloud instances (AWS Graviton, GCP Tau T2A) and Apple Silicon development machines. ARM instances are 20-40% cheaper than x86 equivalents on AWS and GCP. Customers will choose ARM.

### 8. Example Configuration File

A production-ready `hebbs.toml` with annotated commentary is a deliverable, not an afterthought. The example file documents every configuration section with:
- What the parameter controls
- What the default is and why
- When to change it and to what
- What breaks if you set it wrong

This file lives in `hebbs-deploy/examples/` and is referenced by the Helm chart's ConfigMap template as the starting point for customization.

### 9. Model File Strategy: Volume Mount, Not Baked In

The ONNX embedding model (BGE-small-en-v1.5, ~33MB) must be available to the server at startup. Three strategies, in priority order:

1. **Volume mount (recommended for production):** Mount a PVC or hostPath containing the model file at `/models`. Set `HEBBS_EMBEDDING_MODEL_PATH=/models/bge-small-en-v1.5.onnx` in the pod spec. The model file is pre-provisioned (init container downloads it, or it is baked into a separate model image and mounted).
2. **Auto-download (default for evaluation):** HEBBS downloads the model on first start if not found at the configured path. Works out of the box but requires outbound HTTPS to Hugging Face. Not suitable for air-gapped environments.
3. **Init container (recommended for Kubernetes):** A lightweight init container runs before the main container, downloads the model to a shared `emptyDir` volume, and exits. The main container reads from the shared volume. This ensures the model is present before the server starts and avoids first-request latency spikes from model download.

The Helm chart uses strategy 3 by default, with strategy 1 available via values override for air-gapped deployments.

---

## Risk Register

| Risk | Impact | Mitigation |
|------|--------|------------|
| RocksDB C++ runtime incompatible with distroless base image | High -- container fails to start with dynamic linker errors | Verify during build: run `ldd` on the binary in the build stage, confirm all dependencies are present in distroless `cc`. If not, use `debian:bookworm-slim` as fallback (larger but guaranteed compatible). |
| EBS gp3 IOPS insufficient for high write throughput | Medium -- write stalls under load | Document IOPS requirements in Helm chart values and runbook. Terraform module uses gp3 with 3000 IOPS baseline. Alert on `rocksdb_write_stall` metric. |
| HNSW rebuild time exceeds Kubernetes startup probe timeout at large scale | High -- pod enters CrashLoopBackOff | Set startup probe `failureThreshold × periodSeconds` to 5 minutes by default. Document scaling formula: ~30s per 1M memories. Customers at 10M+ memories must increase the timeout. |
| Grafana dashboard becomes stale as new metrics are added in future phases | Medium -- dashboard misses important metrics | Version the dashboard JSON. Include the HEBBS version that the dashboard targets. Document the dashboard update procedure in the runbook. Phase 16 tutorial references the dashboard version. |
| Terraform module becomes incompatible with new AWS provider versions | Medium -- module fails to plan | Pin the AWS provider version range. Include a `versions.tf` with minimum and maximum tested provider versions. |
| Customer runs `replicaCount > 1` expecting horizontal scaling | High -- data divergence, user confusion | Documentation in `values.yaml` explicitly states that replicas are independent instances. Add a startup log warning if the pod detects peer StatefulSet pods without sync enabled. |
| Air-gapped deployment cannot download ONNX model | Medium -- server starts but cannot embed | Document the init-container volume-mount strategy. Provide a script to pre-download the model for offline transfer. |
| Helm chart `helm upgrade` loses RocksDB data due to PVC reclaim policy | Critical -- data loss | Default `reclaimPolicy` to `Retain` (not `Delete`). Document the PVC lifecycle. Warn in values file that changing StorageClass on an existing deployment does not migrate data. |

---

## Deliverables Checklist

Phase 15 is done when ALL of the following are true:

### Docker

- [ ] Multi-stage Dockerfile produces a working container image under 80MB
- [ ] Container runs as non-root user (UID 65532)
- [ ] `docker run hebbs-ai/hebbs` starts a server that responds to health checks within 10 seconds
- [ ] `STOPSIGNAL SIGTERM` and graceful shutdown verified (data survives `docker stop`)
- [ ] `HEALTHCHECK` instruction present and functional
- [ ] Container image is multi-platform (linux/amd64, linux/arm64)

### Helm Chart

- [ ] `helm install hebbs hebbs-deploy/helm/hebbs` deploys a working HEBBS StatefulSet
- [ ] PersistentVolumeClaim created with configurable StorageClass
- [ ] ConfigMap mounts `hebbs.toml`, Secret mounts sensitive environment variables
- [ ] Startup, liveness, and readiness probes are configured and functional
- [ ] ServiceMonitor created when `metrics.serviceMonitor.enabled=true`
- [ ] Ingress resource created when `ingress.enabled=true`, with annotation presets for NGINX and ALB
- [ ] `helm template` renders valid Kubernetes manifests (tested via `helm lint` and `kubeval`)
- [ ] Resource requests and limits set with documented rationale
- [ ] Init container downloads ONNX model to shared volume before server starts
- [ ] `values.yaml` is annotated with parameter descriptions, defaults, and sizing guidance

### Terraform

- [ ] AWS module provisions VPC + EKS + EBS CSI + ALB Controller + Helm release from a single `terraform apply`
- [ ] Module accepts variables for instance type, storage size, VPC configuration, and HEBBS version
- [ ] `terraform destroy` cleanly tears down all resources (including PVCs, which use `Retain` policy -- documented as manual cleanup step)
- [ ] Module outputs: cluster endpoint, ALB DNS name, `kubectl` configuration command

### Monitoring Artifacts

- [ ] Grafana dashboard JSON covers all 5 operational rows (health, latency, capacity, cognitive, security)
- [ ] Grafana dashboard JSON importable into any Grafana instance (tested via manual import)
- [ ] Prometheus alerting rules defined for all 8 alert conditions as a standalone rules file
- [ ] Every alert has a `runbook_url` annotation pointing to the corresponding runbook section
- [ ] Dashboard and alerting rules published in `hebbs-deploy/dashboards/` and `hebbs-deploy/alerts/`

### Runbook

- [ ] Backup and restore procedure documented with step-by-step commands
- [ ] Scaling guide with memory-count-to-instance-type sizing table
- [ ] Upgrade procedure for single-replica and multi-replica deployments
- [ ] Incident response sections for: latency spike, disk full, OOM, reflect stall, auth failure spike
- [ ] Every incident section follows the template: Symptoms → Triage → Root Cause → Resolution → Verification → Prevention

### CI/CD

- [ ] Container image built and pushed to `ghcr.io` on `v*` tag push
- [ ] Multi-platform build (amd64 + arm64) via `docker buildx`
- [ ] Container smoke test passes in CI (start → health check → remember → recall → stop)
- [ ] Image tagged with version, `latest`, and Git SHA

### Configuration

- [ ] Production-ready `hebbs.toml` example with annotated commentary for every section
- [ ] Example configurations for common patterns: minimal, production, high-throughput, air-gapped

---

## Interfaces Published to Future Phases

| Interface | Consumer Phases | Stability Requirement |
|-----------|----------------|----------------------|
| Docker image tag scheme (`ghcr.io/hebbs-ai/hebbs:v*`) | 16 (Documentation -- quick-start), 17 (Edge deployment uses same image with different config) | Stable: tag scheme is a public contract |
| Helm chart values schema | 16 (Documentation), 17 (Edge Helm values profile), customers | Semver-versioned: breaking changes require chart major version bump |
| Terraform module input variables | 16 (Documentation), customers | Semver-versioned: follows Terraform module versioning conventions |
| Grafana dashboard JSON file | 16 (Documentation -- monitoring tutorial imports this file, screenshots reference it), future dashboard revisions | Versioned: dashboard `version` field incremented on structural changes |
| Prometheus alerting rules file | 16 (Documentation -- alerting tutorial references this file), customer alerting pipelines (PagerDuty, Opsgenie integrations key on alert names) | Stable: renaming an alert is a breaking change for customers |
| Runbook section URLs (anchors) | Alert annotations (`runbook_url`), 16 (Documentation -- linked from monitoring tutorial), customer on-call documentation | Stable: changing a URL breaks every alert that references it |
| Container image filesystem layout (`/data`, `/models`, `/config`) | 17 (Edge mode mounts same paths), Helm chart volume mounts | Stable: path changes require Helm chart and documentation updates |
| Init container model download pattern | 17 (Edge deployment downloads different model sizes) | Reusable pattern: same init container image, different model URL |

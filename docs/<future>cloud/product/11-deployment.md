# HEBBS Cloud: Deployment

## Overview

HEBBS Cloud runs on Kubernetes. The central platform runs in one region (US). Regional infrastructure runs in each supported region. Tenant containers are the unmodified `hebbs` binary in a Docker image.

---

## Container image

### hebbs-server image

Built from the existing `hebbs` repo. No code changes.

```dockerfile
FROM rust:1.75-slim AS builder
WORKDIR /build
COPY . .
RUN cargo build --release --bin hebbs

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/hebbs /usr/local/bin/hebbs
EXPOSE 6380 6381
ENTRYPOINT ["hebbs"]
CMD ["start", "--bind-rest", "0.0.0.0:6381", "--bind-grpc", "0.0.0.0:6380"]
```

**Image size:** ~50MB (Rust static binary + minimal Debian)
**No ONNX runtime included** — cloud uses OpenAI embeddings, not local inference.

Build feature flags:
```sh
cargo build --release --no-default-features --features "server,openai-embed,openai-llm"
```

This excludes ONNX runtime, reducing image size to ~20MB.

### hebbs-platform image

The new platform service. Single binary, two modes.

```dockerfile
FROM rust:1.75-slim AS builder
WORKDIR /build
COPY . .
RUN cargo build --release --bin hebbs-platform

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/hebbs-platform /usr/local/bin/hebbs-platform
EXPOSE 8080
ENTRYPOINT ["hebbs-platform"]
```

Runs in two modes:
```sh
hebbs-platform --mode central    # auth, billing, workspace manager, global gateway
hebbs-platform --mode regional   # regional gateway, file upload, usage metering
```

---

## Kubernetes architecture

### Central cluster (US)

```yaml
Namespace: hebbs-central
│
├── Deployment: platform-central
│   ├── Replicas: 2 (HA)
│   ├── Image: hebbs-platform:latest
│   ├── Args: ["--mode", "central"]
│   ├── Env:
│   │   ├── DATABASE_URL: postgres://...
│   │   ├── STRIPE_SECRET_KEY: sk_live_...
│   │   ├── OPENAI_API_KEY: sk-proj-...
│   │   └── REGION_REGISTRY: '{"us":"us.api.hebbs.ai","eu":"eu.api.hebbs.ai"}'
│   └── Resources: 1 vCPU, 1Gi memory
│
├── Deployment: platform-regional-us
│   ├── Replicas: 2 (HA)
│   ├── Image: hebbs-platform:latest
│   ├── Args: ["--mode", "regional"]
│   ├── Env:
│   │   ├── REGION: us
│   │   ├── CENTRAL_URL: http://platform-central:8080
│   │   └── OPENAI_API_KEY: sk-proj-...
│   └── Resources: 1 vCPU, 1Gi memory
│
├── Service: api-global (LoadBalancer → platform-central)
│   └── External: api.hebbs.ai
│
├── Service: api-us (LoadBalancer → platform-regional-us)
│   └── External: us.api.hebbs.ai
│
├── StatefulSet: postgres
│   ├── Replicas: 1 (+ read replica for HA)
│   ├── Storage: 50Gi PVC
│   └── Backup: daily snapshot
│
└── Workspace containers (dynamic)
    ├── ws-support-agent (Deployment + PVC + ClusterIP Service)
    ├── ws-internal-kb
    └── ...
```

### EU cluster

```yaml
Namespace: hebbs-eu
│
├── Deployment: platform-regional-eu
│   ├── Replicas: 2 (HA)
│   ├── Image: hebbs-platform:latest
│   ├── Args: ["--mode", "regional"]
│   ├── Env:
│   │   ├── REGION: eu
│   │   ├── CENTRAL_URL: https://api.hebbs.ai (internal)
│   │   └── OPENAI_API_KEY: sk-proj-...
│   └── Resources: 1 vCPU, 1Gi memory
│
├── Service: api-eu (LoadBalancer → platform-regional-eu)
│   └── External: eu.api.hebbs.ai
│
└── Workspace containers (dynamic)
    ├── ws-sales-agent
    ├── ws-support-eu
    └── ...
```

---

## Workspace container provisioning

When the regional gateway provisions a new workspace:

### Kubernetes resources created

**1. PersistentVolumeClaim**

```yaml
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: ws-{slug}-data
  namespace: hebbs-{region}
spec:
  accessModes: [ReadWriteOnce]
  resources:
    requests:
      storage: 1Gi  # scales with plan
  storageClassName: gp3-encrypted  # encrypted at rest
```

**2. Deployment**

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: ws-{slug}
  namespace: hebbs-{region}
  labels:
    app: hebbs-workspace
    workspace: "{slug}"
spec:
  replicas: 1
  selector:
    matchLabels:
      workspace: "{slug}"
  template:
    metadata:
      labels:
        workspace: "{slug}"
    spec:
      containers:
      - name: hebbs
        image: hebbs-server:latest
        args: ["start", "--bind-rest", "0.0.0.0:6381", "--bind-grpc", "0.0.0.0:6380"]
        env:
        - name: HEBBS_LLM_PROVIDER
          value: "openai"
        - name: HEBBS_LLM_MODEL
          value: "gpt-4o-mini"
        - name: HEBBS_LLM_API_KEY
          valueFrom:
            secretKeyRef:
              name: openai-credentials
              key: api-key
        - name: HEBBS_EMBED_PROVIDER
          value: "openai"
        - name: HEBBS_EMBED_MODEL
          value: "text-embedding-3-small"
        - name: HEBBS_EMBED_API_KEY
          valueFrom:
            secretKeyRef:
              name: openai-credentials
              key: api-key
        - name: HEBBS_EMBED_DIMENSIONS
          value: "1536"
        ports:
        - containerPort: 6381
          name: rest
        - containerPort: 6380
          name: grpc
        volumeMounts:
        - name: data
          mountPath: /data
        resources:
          requests:
            cpu: 250m
            memory: 256Mi
          limits:
            cpu: 500m
            memory: 512Mi
        readinessProbe:
          httpGet:
            path: /v1/health/ready
            port: 6381
          initialDelaySeconds: 2
          periodSeconds: 10
        livenessProbe:
          httpGet:
            path: /v1/health/live
            port: 6381
          initialDelaySeconds: 5
          periodSeconds: 30
      volumes:
      - name: data
        persistentVolumeClaim:
          claimName: ws-{slug}-data
```

**3. Service (ClusterIP, internal only)**

```yaml
apiVersion: v1
kind: Service
metadata:
  name: ws-{slug}
  namespace: hebbs-{region}
spec:
  selector:
    workspace: "{slug}"
  ports:
  - name: rest
    port: 6381
  - name: grpc
    port: 6380
  type: ClusterIP
```

### Init sequence

After the pod is ready:
1. Regional gateway calls `POST ws-{slug}:6381/v1/health/ready` to confirm
2. Regional gateway runs `hebbs init /data` inside the container (via Kubernetes exec or init container)
3. Workspace status updated to `active` in platform DB

### Suspension (free tier, 30d inactivity)

```sh
kubectl scale deployment ws-{slug} --replicas=0 -n hebbs-{region}
```

Volume preserved. Container stopped. Cost drops to volume storage only (~$0.10/month).

### Resume

```sh
kubectl scale deployment ws-{slug} --replicas=1 -n hebbs-{region}
```

Cold start: ~2-3 seconds (RocksDB opens, daemon starts, health check passes).

### Deletion

```sh
kubectl delete deployment ws-{slug} -n hebbs-{region}
kubectl delete service ws-{slug} -n hebbs-{region}
# PVC retained for 30 days, then deleted
kubectl delete pvc ws-{slug}-data -n hebbs-{region}  # after 30d
```

---

## Scaling considerations

### Tenants per node

| Resource | Per workspace | Per node (8 vCPU, 32Gi) | Workspaces per node |
|---|---|---|---|
| CPU (request) | 250m | 8000m | 32 |
| Memory (request) | 256Mi | 32Gi | 128 |
| **Bottleneck** | CPU | | **~32 tenants per node** |

Most tenants are idle most of the time (bursty recall traffic). Real density is higher with overcommit.

### Node autoscaling

- Min nodes per region: 2 (HA)
- Max nodes per region: 50 (initial)
- Scale trigger: CPU utilization >70% across node group
- Scale-down: after 10 minutes of <30% utilization

### Storage

- Volume type: gp3 (encrypted, SSD)
- Initial size: 1Gi per workspace
- Growth: monitor via Prometheus, alert at 80%, expand PVC

---

## Monitoring

### Prometheus metrics

**Platform metrics (from gateway):**
- `hebbs_requests_total{workspace, method, status}` — request count
- `hebbs_request_duration_seconds{workspace, method}` — latency histogram
- `hebbs_active_tenants{region}` — gauge of running containers
- `hebbs_usage_recalls_total{workspace}` — recall count for billing
- `hebbs_usage_memories_total{workspace}` — memory count for billing

**Tenant metrics (from hebbs-server, already exists):**
- `hebbs_recall_duration_seconds` — per-query latency
- `hebbs_memories_total` — memory count
- `hebbs_index_files_total` — indexed file count
- `hebbs_reflection_duration_seconds` — reflection cycle time

### Alerting

| Alert | Condition | Severity |
|---|---|---|
| Tenant unhealthy | Liveness probe fails 3x | Critical |
| High recall latency | p99 > 500ms for 5 minutes | Warning |
| Storage near full | PVC > 80% capacity | Warning |
| Node capacity low | <20% CPU headroom | Warning |
| Gateway error rate | 5xx rate > 1% for 5 minutes | Critical |
| OpenAI API errors | Error rate > 5% for 5 minutes | Critical |

### Logging

- Structured JSON logs (already implemented in hebbs-server)
- Shipped to centralized logging (CloudWatch, Datadog, or similar)
- Tenant ID included in all log lines for filtering
- Log retention: 30 days

---

## Backup and disaster recovery

### Volume snapshots

- Daily automated snapshots of all workspace PVCs
- Retention: 7 daily, 4 weekly
- Restore: create new PVC from snapshot, attach to new deployment

### Platform database

- Postgres: daily pg_dump + point-in-time recovery (WAL archiving)
- Retention: 30 days
- RPO: 1 hour (WAL shipping interval)
- RTO: 30 minutes (restore from snapshot + replay WAL)

### Cross-region (future)

Not at launch. Data exists in one region only. If a region goes down, workspaces in that region are unavailable until the region recovers.

**Future:** Async replication of RocksDB volumes to a standby region. Adds complexity and cost. Only justified when SLA demands it (Enterprise tier).

---

## CI/CD pipeline

```
Push to hebbs repo (engine)
  → Build hebbs-server image
  → Push to container registry
  → Rolling update of workspace containers (canary: 1 workspace first, then all)

Push to hebbs-platform repo
  → Build hebbs-platform image
  → Push to container registry
  → Rolling update of platform services (zero-downtime)
```

Tenant container updates are rolling — each container is restarted one at a time. RocksDB recovers from WAL on restart. No data loss.

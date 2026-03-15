# hebbs-deploy

Deployment, monitoring, and operations toolkit for [HEBBS](https://github.com/hebbs-ai/hebbs) — the cognitive memory engine for AI agents.

This repository contains everything needed to install, deploy, observe, and operate HEBBS — from a single `curl` command on a laptop to production Kubernetes clusters.

---

## Repository Structure

```
hebbs-deploy/
├── scripts/            Installer scripts (curl | sh)
├── helm/               Helm chart for Kubernetes deployment
├── terraform/          Terraform modules for infrastructure provisioning
├── dashboards/         Grafana dashboards (importable JSON)
├── alerts/             Prometheus alerting rules
├── runbooks/           Operational runbooks and incident procedures
└── examples/           Example configurations and deployment manifests
```

| Directory      | Contents                                                                 |
|----------------|--------------------------------------------------------------------------|
| `scripts/`     | `install.sh` — curl-pipe installer for macOS and Linux binaries          |
| `helm/`        | Helm chart with configurable values for HEBBS StatefulSet, PVCs, RBAC    |
| `terraform/`   | Terraform modules for cloud infrastructure (VPC, EKS/GKE, storage)       |
| `dashboards/`  | Grafana dashboard JSON files — import via UI or provisioning             |
| `alerts/`      | Prometheus `rules.yml` files with alert definitions and runbook links     |
| `runbooks/`    | Operations runbook: backup/restore, scaling, upgrades, incident response |
| `examples/`    | Example values files, Docker Compose setups, and standalone configs       |

---

## Quick Start: Binary Install

The fastest way to get HEBBS running on a single machine.

```bash
curl -sSf https://hebbs.ai/install | sh
```

This downloads the pre-built `hebbs` binary from GitHub Releases with SHA-256 checksum verification.

| Variable | Description |
|----------|-------------|
| `HEBBS_VERSION` | Pin a specific version (e.g. `v0.5.0`). Default: latest. |
| `HEBBS_INSTALL_DIR` | Override install directory. Default: `~/.hebbs/bin` (non-root) or `/usr/local/bin` (root). |
| `HEBBS_NO_VERIFY` | Set to `1` to skip checksum verification. |

```bash
# Install a specific version
HEBBS_VERSION=v0.5.0 curl -sSf https://hebbs.ai/install | sh

# Install to a custom directory
HEBBS_INSTALL_DIR=/opt/hebbs/bin curl -sSf https://hebbs.ai/install | sh
```

Supported platforms: Linux x86_64, Linux aarch64, macOS arm64 (Apple Silicon).

---

## Quick Start: Helm

### Prerequisites

- Kubernetes 1.27+
- Helm 3.12+
- A Prometheus + Grafana stack (e.g., kube-prometheus-stack)

### Install

```bash
# Add the HEBBS Helm repo
helm repo add hebbs https://charts.hebbs.ai
helm repo update

# Install with default values
helm install hebbs hebbs/hebbs \
  --namespace hebbs \
  --create-namespace

# Or install from this repository directly
helm install hebbs ./helm \
  --namespace hebbs \
  --create-namespace \
  -f examples/values-production.yaml
```

### Verify

```bash
kubectl -n hebbs get pods
kubectl -n hebbs port-forward svc/hebbs 6381:6381 50051:50051

# Health check
curl http://localhost:6381/v1/health

# Metrics
curl http://localhost:6381/v1/metrics
```

### Configure

See `helm/values.yaml` for the full list of configurable parameters. Key values:

```yaml
resources:
  requests:
    memory: "512Mi"
    cpu: "1"
  limits:
    memory: "1Gi"
    cpu: "2"

persistence:
  size: 50Gi
  storageClass: gp3

config:
  blockCacheMb: 256
  hnswEfSearch: 100
  reflectEnabled: true
  reflectIntervalSecs: 3600
```

---

## Quick Start: Terraform

### Prerequisites

- Terraform 1.5+
- Cloud provider CLI configured (AWS, GCP, or Azure)

### Deploy

```bash
cd terraform/

# Initialize and plan
terraform init
terraform plan -var-file=environments/production.tfvars

# Apply
terraform apply -var-file=environments/production.tfvars
```

The Terraform modules provision:
- Kubernetes cluster (EKS/GKE/AKS)
- Persistent volume storage with appropriate IOPS
- Network policies and load balancer
- Monitoring namespace with Prometheus + Grafana

After Terraform completes, deploy HEBBS with Helm using the generated kubeconfig.

---

## Monitoring

### Grafana Dashboards

Import dashboards from the `dashboards/` directory:

| Dashboard                                         | Description                                              |
|---------------------------------------------------|----------------------------------------------------------|
| [`hebbs-overview.json`](dashboards/hebbs-overview.json) | Service health, latency, capacity, cognitive health, errors |

**Import via Grafana UI:**
1. Open Grafana → Dashboards → Import
2. Upload the JSON file or paste its contents
3. Select your Prometheus datasource
4. Click Import

**Import via provisioning:**
```yaml
# grafana-provisioning/dashboards/hebbs.yaml
apiVersion: 1
providers:
  - name: HEBBS
    folder: HEBBS
    type: file
    options:
      path: /var/lib/grafana/dashboards/hebbs
```

### Prometheus Alerts

Load alerting rules from the `alerts/` directory:

| File                                      | Rules                                                                  |
|-------------------------------------------|------------------------------------------------------------------------|
| [`hebbs-alerts.yml`](alerts/hebbs-alerts.yml) | 8 rules: down, latency, errors, disk, reflect, compaction, auth   |

**Load via Prometheus config:**
```yaml
# prometheus.yml
rule_files:
  - /etc/prometheus/rules/hebbs-alerts.yml
```

**Load via Prometheus Operator:**
```yaml
apiVersion: monitoring.coreos.com/v1
kind: PrometheusRule
metadata:
  name: hebbs-alerts
  labels:
    release: kube-prometheus-stack
spec:
  # Paste the contents of alerts/hebbs-alerts.yml here
```

---

## Operations

The [`runbooks/operations.md`](runbooks/operations.md) contains comprehensive procedures for:

- **Backup and Restore** — RocksDB checkpoint-based, with kubectl commands
- **Scaling Guide** — RAM sizing formula and sizing table (100K to 10M memories)
- **Upgrade Procedure** — Single-replica and multi-replica rolling upgrades
- **Incident Response** — 7 incident runbooks with triage, root cause, resolution, and prevention

Every Prometheus alert links to its corresponding runbook section via the `runbook_url` annotation.

---

## HEBBS Endpoints

| Protocol | Port  | Path           | Description                        |
|----------|-------|----------------|------------------------------------|
| gRPC     | 50051 | —              | Primary API for all memory operations |
| HTTP     | 6381  | `/v1/health`   | Health check (JSON)                |
| HTTP     | 6381  | `/v1/metrics`  | Prometheus metrics endpoint        |

---

## License

Deployment tooling in this repository is licensed under Apache 2.0. HEBBS itself is licensed under BSL 1.1 — see the [HEBBS repository](https://github.com/hebbs-ai/hebbs) for details.

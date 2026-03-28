# HEBBS Cloud: AWS Deployment

## Domains

```
hebbs.ai                    → Marketing website (existing)
app.hebbs.ai                → Web console (React SPA)
api.hebbs.ai                → Global gateway (central, US)
us.api.hebbs.ai             → US regional gateway
eu.api.hebbs.ai             → EU regional gateway
status.hebbs.ai             → Status page
```

All subdomains via Route 53. TLS via ACM (AWS Certificate Manager).

---

## AWS Architecture

```
                    Route 53
                    ┌──────────────────────────────────┐
                    │ api.hebbs.ai    → US ALB          │
                    │ us.api.hebbs.ai → US ALB          │
                    │ eu.api.hebbs.ai → EU ALB          │
                    │ app.hebbs.ai    → CloudFront      │
                    └──────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│  US-EAST-1 (Virginia) — Central + US Region                  │
│                                                              │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │  ALB (Application Load Balancer)                         │ │
│  │  api.hebbs.ai + us.api.hebbs.ai                         │ │
│  │  TLS termination (ACM cert)                              │ │
│  │                                                          │ │
│  │  Rules:                                                  │ │
│  │  api.hebbs.ai    → target: platform-central              │ │
│  │  us.api.hebbs.ai → target: platform-regional-us          │ │
│  └──────────┬──────────────────────┬────────────────────────┘ │
│             │                      │                          │
│             ▼                      ▼                          │
│  ┌──────────────────┐  ┌──────────────────────┐              │
│  │ EKS Cluster       │  │ RDS (Postgres)       │              │
│  │ "hebbs-central"   │  │ db.t4g.medium        │              │
│  │                   │  │ Multi-AZ             │              │
│  │ Namespaces:       │  │ 50GB gp3             │              │
│  │ ├─ hebbs-central  │  │ Daily snapshots      │              │
│  │ └─ hebbs-us       │  │ Point-in-time recovery│             │
│  │                   │  └──────────────────────┘              │
│  │ Node group:       │                                        │
│  │ ├─ platform:      │  ┌──────────────────────┐              │
│  │ │  2x t3.medium   │  │ ECR (Container Reg)  │              │
│  │ └─ workspaces:    │  │ hebbs-server:latest   │              │
│  │    2-50x t3.large │  │ hebbs-platform:latest │              │
│  │    (autoscale)    │  └──────────────────────┘              │
│  └───────────────────┘                                        │
│                                                              │
│  ┌──────────────────────┐  ┌──────────────────────┐          │
│  │ CloudFront            │  │ S3                    │          │
│  │ app.hebbs.ai          │  │ console SPA assets    │          │
│  │ → S3 origin            │  │ (index.html, js, css) │         │
│  └──────────────────────┘  └──────────────────────┘          │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│  EU-WEST-1 (Ireland) — EU Region                             │
│                                                              │
│  ┌──────────────────────────────────────────────────────────┐│
│  │  ALB                                                      ││
│  │  eu.api.hebbs.ai                                          ││
│  │  TLS termination (ACM cert)                               ││
│  └──────────┬───────────────────────────────────────────────┘│
│             │                                                │
│             ▼                                                │
│  ┌──────────────────┐                                        │
│  │ EKS Cluster       │                                        │
│  │ "hebbs-eu"        │                                        │
│  │                   │                                        │
│  │ Namespace:        │                                        │
│  │ └─ hebbs-eu       │                                        │
│  │                   │                                        │
│  │ Node group:       │                                        │
│  │ ├─ platform:      │                                        │
│  │ │  2x t3.medium   │                                        │
│  │ └─ workspaces:    │                                        │
│  │    2-50x t3.large │                                        │
│  │    (autoscale)    │                                        │
│  └───────────────────┘                                        │
└──────────────────────────────────────────────────────────────┘
```

---

## AWS Services Used

| Service | Purpose | Cost estimate (starting) |
|---|---|---|
| **EKS** (x2 clusters) | Kubernetes for platform + workspace containers | $144/mo (2 x $72 control plane) |
| **EC2 (node groups)** | Run containers. t3.medium for platform, t3.large for workspaces | ~$200/mo (4 platform nodes + 4 workspace nodes) |
| **RDS Postgres** | Platform DB (orgs, keys, billing, usage) | ~$70/mo (db.t4g.medium, Multi-AZ) |
| **ALB** (x2) | Load balancing + TLS termination | ~$40/mo (2 x $20) |
| **ECR** | Container image registry | ~$5/mo |
| **Route 53** | DNS | ~$1/mo |
| **ACM** | TLS certificates | Free |
| **CloudFront + S3** | Console SPA hosting | ~$5/mo |
| **EBS gp3** | Persistent volumes for workspace RocksDB | ~$0.08/GB/mo per workspace |
| **Secrets Manager** | OpenAI key, Stripe key, DB credentials | ~$5/mo |
| **CloudWatch** | Logs, metrics, alerts | ~$20/mo |
| **Total (starting)** | 2 regions, ~10 workspaces | **~$500/mo** |

Scales roughly linearly with workspaces. Each additional workspace adds ~$12/mo (container compute + storage).

---

## Step-by-step deployment

### 1. Prerequisites

```sh
# Tools needed
aws cli          # configured with admin access
eksctl           # EKS cluster management
kubectl          # k8s operations
helm             # package manager for k8s
docker           # build images
terraform        # optional, for IaC (or use eksctl + aws cli)
```

### 2. DNS and certificates

```sh
# Route 53 — create hosted zone (if not already)
# hebbs.ai is already registered, add subdomains

# ACM — request certificates
# US cert (for api.hebbs.ai, us.api.hebbs.ai, app.hebbs.ai)
aws acm request-certificate \
  --domain-name "api.hebbs.ai" \
  --subject-alternative-names "us.api.hebbs.ai" "app.hebbs.ai" \
  --validation-method DNS \
  --region us-east-1

# EU cert (for eu.api.hebbs.ai)
aws acm request-certificate \
  --domain-name "eu.api.hebbs.ai" \
  --validation-method DNS \
  --region eu-west-1

# Validate via DNS (add CNAME records ACM provides to Route 53)
```

### 3. Container images

```sh
# Create ECR repositories
aws ecr create-repository --repository-name hebbs-server --region us-east-1
aws ecr create-repository --repository-name hebbs-platform --region us-east-1

# Build and push hebbs-server (from hebbs/ repo, unchanged)
cd hebbs/
docker build -t hebbs-server:latest \
  --build-arg FEATURES="server,openai-embed,openai-llm" .
docker tag hebbs-server:latest <account>.dkr.ecr.us-east-1.amazonaws.com/hebbs-server:latest
docker push <account>.dkr.ecr.us-east-1.amazonaws.com/hebbs-server:latest

# Build and push hebbs-platform (from hebbs-platform/ repo)
cd hebbs-platform/
docker build -t hebbs-platform:latest .
docker tag hebbs-platform:latest <account>.dkr.ecr.us-east-1.amazonaws.com/hebbs-platform:latest
docker push <account>.dkr.ecr.us-east-1.amazonaws.com/hebbs-platform:latest

# Replicate images to EU region
aws ecr create-repository --repository-name hebbs-server --region eu-west-1
aws ecr create-repository --repository-name hebbs-platform --region eu-west-1
# Use ECR replication or push to both
```

### 4. EKS cluster — US (central + regional)

```sh
# Create cluster
eksctl create cluster \
  --name hebbs-central \
  --region us-east-1 \
  --version 1.29 \
  --nodegroup-name platform \
  --node-type t3.medium \
  --nodes 2 \
  --nodes-min 2 \
  --nodes-max 4

# Add workspace node group (separate, autoscaling)
eksctl create nodegroup \
  --cluster hebbs-central \
  --name workspaces \
  --node-type t3.large \
  --nodes 2 \
  --nodes-min 2 \
  --nodes-max 50 \
  --node-labels "role=workspace"

# Install EBS CSI driver (for persistent volumes)
eksctl create addon \
  --cluster hebbs-central \
  --name aws-ebs-csi-driver \
  --region us-east-1

# Create encrypted storage class for workspace volumes
kubectl apply -f - <<EOF
apiVersion: storage.k8s.io/v1
kind: StorageClass
metadata:
  name: gp3-encrypted
provisioner: ebs.csi.aws.com
parameters:
  type: gp3
  encrypted: "true"
reclaimPolicy: Retain
volumeBindingMode: WaitForFirstConsumer
EOF

# Create namespaces
kubectl create namespace hebbs-central
kubectl create namespace hebbs-us
```

### 5. Secrets

```sh
# Store secrets in AWS Secrets Manager
aws secretsmanager create-secret \
  --name hebbs/openai-api-key \
  --secret-string "sk-proj-your-openai-key"

aws secretsmanager create-secret \
  --name hebbs/stripe-secret-key \
  --secret-string "sk_live_your-stripe-key"

aws secretsmanager create-secret \
  --name hebbs/db-password \
  --secret-string "your-db-password"

# Install External Secrets Operator (syncs AWS secrets → k8s secrets)
helm repo add external-secrets https://charts.external-secrets.io
helm install external-secrets external-secrets/external-secrets \
  -n external-secrets --create-namespace

# Create SecretStore + ExternalSecret resources
# (these pull from AWS Secrets Manager into k8s Secret objects)
kubectl apply -f k8s/secrets/
```

### 6. RDS Postgres

```sh
# Create subnet group (use EKS VPC's private subnets)
aws rds create-db-subnet-group \
  --db-subnet-group-name hebbs-db \
  --subnet-ids subnet-xxx subnet-yyy \
  --db-subnet-group-description "HEBBS platform DB"

# Create Postgres instance
aws rds create-db-instance \
  --db-instance-identifier hebbs-platform-db \
  --db-instance-class db.t4g.medium \
  --engine postgres \
  --engine-version 16.4 \
  --allocated-storage 50 \
  --storage-type gp3 \
  --storage-encrypted \
  --multi-az \
  --db-name hebbs \
  --master-username hebbs_admin \
  --master-user-password <from-secrets-manager> \
  --vpc-security-group-ids sg-xxx \
  --db-subnet-group-name hebbs-db \
  --backup-retention-period 7 \
  --preferred-backup-window "03:00-04:00"

# Security group: allow inbound 5432 from EKS node security group only
```

### 7. Deploy platform — central

```yaml
# k8s/central/deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: platform-central
  namespace: hebbs-central
spec:
  replicas: 2
  selector:
    matchLabels:
      app: platform-central
  template:
    metadata:
      labels:
        app: platform-central
    spec:
      nodeSelector:
        role: platform
      containers:
      - name: platform
        image: <account>.dkr.ecr.us-east-1.amazonaws.com/hebbs-platform:latest
        args: ["--mode", "central"]
        env:
        - name: DATABASE_URL
          valueFrom:
            secretKeyRef:
              name: hebbs-secrets
              key: database-url
        - name: STRIPE_SECRET_KEY
          valueFrom:
            secretKeyRef:
              name: hebbs-secrets
              key: stripe-secret-key
        - name: REGION_REGISTRY
          value: '{"us":"http://platform-regional-us.hebbs-us:8080","eu":"https://eu.api.hebbs.ai"}'
        ports:
        - containerPort: 8080
        resources:
          requests:
            cpu: 500m
            memory: 512Mi
          limits:
            cpu: 1000m
            memory: 1Gi
        readinessProbe:
          httpGet:
            path: /health
            port: 8080
          initialDelaySeconds: 5
---
apiVersion: v1
kind: Service
metadata:
  name: platform-central
  namespace: hebbs-central
spec:
  selector:
    app: platform-central
  ports:
  - port: 8080
  type: ClusterIP
```

### 8. Deploy platform — US regional gateway

```yaml
# k8s/us/deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: platform-regional-us
  namespace: hebbs-us
spec:
  replicas: 2
  selector:
    matchLabels:
      app: platform-regional-us
  template:
    metadata:
      labels:
        app: platform-regional-us
    spec:
      nodeSelector:
        role: platform
      containers:
      - name: platform
        image: <account>.dkr.ecr.us-east-1.amazonaws.com/hebbs-platform:latest
        args: ["--mode", "regional"]
        env:
        - name: REGION
          value: "us"
        - name: CENTRAL_URL
          value: "http://platform-central.hebbs-central:8080"
        - name: OPENAI_API_KEY
          valueFrom:
            secretKeyRef:
              name: hebbs-secrets
              key: openai-api-key
        ports:
        - containerPort: 8080
        resources:
          requests:
            cpu: 500m
            memory: 512Mi
---
apiVersion: v1
kind: Service
metadata:
  name: platform-regional-us
  namespace: hebbs-us
spec:
  selector:
    app: platform-regional-us
  ports:
  - port: 8080
  type: ClusterIP
```

### 9. ALB (Ingress)

```yaml
# Install AWS Load Balancer Controller
helm repo add eks https://aws.github.io/eks-charts
helm install aws-load-balancer-controller eks/aws-load-balancer-controller \
  -n kube-system \
  --set clusterName=hebbs-central

# k8s/ingress.yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: hebbs-ingress
  namespace: hebbs-central
  annotations:
    kubernetes.io/ingress.class: alb
    alb.ingress.kubernetes.io/scheme: internet-facing
    alb.ingress.kubernetes.io/certificate-arn: arn:aws:acm:us-east-1:<account>:certificate/xxx
    alb.ingress.kubernetes.io/listen-ports: '[{"HTTPS":443}]'
    alb.ingress.kubernetes.io/ssl-redirect: "443"
spec:
  rules:
  - host: api.hebbs.ai
    http:
      paths:
      - path: /
        pathType: Prefix
        backend:
          service:
            name: platform-central
            port:
              number: 8080
  - host: us.api.hebbs.ai
    http:
      paths:
      - path: /
        pathType: Prefix
        backend:
          service:
            name: platform-regional-us
            port:
              number: 8080
```

### 10. Console (SPA)

```sh
# Create S3 bucket for SPA
aws s3 mb s3://hebbs-console --region us-east-1

# Build and deploy console
cd hebbs-platform/console/
npm run build
aws s3 sync dist/ s3://hebbs-console/

# Create CloudFront distribution
aws cloudfront create-distribution \
  --origin-domain-name hebbs-console.s3.amazonaws.com \
  --default-root-object index.html \
  --aliases app.hebbs.ai \
  --viewer-certificate AcmCertificateArn=arn:aws:acm:us-east-1:<account>:certificate/xxx

# Route 53: app.hebbs.ai → CloudFront distribution
```

### 11. EKS cluster — EU

```sh
# Create EU cluster
eksctl create cluster \
  --name hebbs-eu \
  --region eu-west-1 \
  --version 1.29 \
  --nodegroup-name platform \
  --node-type t3.medium \
  --nodes 2

eksctl create nodegroup \
  --cluster hebbs-eu \
  --name workspaces \
  --node-type t3.large \
  --nodes 2 \
  --nodes-min 2 \
  --nodes-max 50 \
  --node-labels "role=workspace"

# Same setup: EBS CSI, storage class, secrets, namespace
kubectl create namespace hebbs-eu

# Deploy regional gateway (same yaml as US, different REGION env)
# Deploy ALB ingress for eu.api.hebbs.ai
```

### 12. Route 53 DNS records

```sh
# api.hebbs.ai → US ALB
aws route53 change-resource-record-sets --hosted-zone-id Z123 --change-batch '{
  "Changes": [{"Action":"CREATE","ResourceRecordSet":{
    "Name":"api.hebbs.ai","Type":"A",
    "AliasTarget":{"HostedZoneId":"Z35SXDOTRQ7X7K","DNSName":"k8s-xxx.us-east-1.elb.amazonaws.com","EvaluateTargetHealth":true}
  }}]}'

# us.api.hebbs.ai → US ALB (same ALB, different host rule)
aws route53 change-resource-record-sets --hosted-zone-id Z123 --change-batch '{
  "Changes": [{"Action":"CREATE","ResourceRecordSet":{
    "Name":"us.api.hebbs.ai","Type":"A",
    "AliasTarget":{"HostedZoneId":"Z35SXDOTRQ7X7K","DNSName":"k8s-xxx.us-east-1.elb.amazonaws.com","EvaluateTargetHealth":true}
  }}]}'

# eu.api.hebbs.ai → EU ALB
aws route53 change-resource-record-sets --hosted-zone-id Z123 --change-batch '{
  "Changes": [{"Action":"CREATE","ResourceRecordSet":{
    "Name":"eu.api.hebbs.ai","Type":"A",
    "AliasTarget":{"HostedZoneId":"Z32O12XQLNTSW2","DNSName":"k8s-xxx.eu-west-1.elb.amazonaws.com","EvaluateTargetHealth":true}
  }}]}'

# app.hebbs.ai → CloudFront
aws route53 change-resource-record-sets --hosted-zone-id Z123 --change-batch '{
  "Changes": [{"Action":"CREATE","ResourceRecordSet":{
    "Name":"app.hebbs.ai","Type":"A",
    "AliasTarget":{"HostedZoneId":"Z2FDTNDATAQYW2","DNSName":"d123xxx.cloudfront.net","EvaluateTargetHealth":false}
  }}]}'
```

---

## How a request flows through AWS

### `POST api.hebbs.ai/v1/recall`

```
1. DNS: api.hebbs.ai → Route 53 → US ALB IP
2. ALB: TLS termination (ACM cert), host rule matches api.hebbs.ai
   → forwards to platform-central Service (k8s ClusterIP)
3. Platform-central pod:
   - Validates API key (cache hit or Postgres lookup)
   - Resolves workspace → region=us
   - Forwards to platform-regional-us Service
4. Platform-regional-us pod:
   - Looks up workspace → container endpoint (ws-support-agent.hebbs-us:6381)
   - Forwards request
5. ws-support-agent pod:
   - Embeds query via OpenAI API (outbound HTTPS from us-east-1)
   - HNSW search in RocksDB (local EBS volume)
   - Returns results
6. Response flows back: pod → regional → central → ALB → customer
```

### `POST us.api.hebbs.ai/v1/recall` (direct regional)

```
1. DNS: us.api.hebbs.ai → Route 53 → US ALB IP
2. ALB: host rule matches us.api.hebbs.ai
   → forwards to platform-regional-us Service (skips central)
3. Platform-regional-us pod:
   - Validates API key directly (cached from central)
   - Routes to workspace container
4. ws-support-agent pod: same as above
5. Response: pod → regional → ALB → customer (one fewer hop)
```

---

## Monitoring setup

### CloudWatch

```sh
# Install CloudWatch agent via Helm
helm repo add aws https://aws.github.io/eks-charts
helm install cloudwatch-agent aws/aws-cloudwatch-observability \
  --set clusterName=hebbs-central \
  --set region=us-east-1

# Container insights enabled — CPU, memory, network per pod
# Logs shipped to CloudWatch Log Groups:
#   /aws/eks/hebbs-central/workloads
#   /aws/eks/hebbs-eu/workloads
```

### Alerts

```sh
# Workspace container unhealthy
aws cloudwatch put-metric-alarm \
  --alarm-name "workspace-unhealthy" \
  --metric-name "pod_status_failed" \
  --namespace "ContainerInsights" \
  --statistic Maximum \
  --period 60 \
  --threshold 1 \
  --comparison-operator GreaterThanOrEqualToThreshold \
  --evaluation-periods 3 \
  --alarm-actions arn:aws:sns:us-east-1:<account>:hebbs-alerts

# High API latency
# RDS CPU > 80%
# Node group capacity < 20% headroom
# EBS volume > 80% full
```

### Prometheus + Grafana (optional, richer than CloudWatch)

```sh
# Install Prometheus stack
helm repo add prometheus-community https://prometheus-community.github.io/helm-charts
helm install monitoring prometheus-community/kube-prometheus-stack \
  -n monitoring --create-namespace

# hebbs-server already exposes /v1/metrics (Prometheus format)
# Add ServiceMonitor to scrape workspace containers
```

---

## CI/CD pipeline

### GitHub Actions

```yaml
# .github/workflows/deploy.yml
name: Deploy HEBBS Cloud

on:
  push:
    branches: [main]

jobs:
  build-and-push:
    runs-on: ubuntu-latest
    steps:
    - uses: actions/checkout@v4

    - name: Configure AWS credentials
      uses: aws-actions/configure-aws-credentials@v4
      with:
        aws-access-key-id: ${{ secrets.AWS_ACCESS_KEY_ID }}
        aws-secret-access-key: ${{ secrets.AWS_SECRET_ACCESS_KEY }}
        aws-region: us-east-1

    - name: Login to ECR
      uses: aws-actions/amazon-ecr-login@v2

    - name: Build and push
      run: |
        docker build -t hebbs-platform:${{ github.sha }} .
        docker tag hebbs-platform:${{ github.sha }} <account>.dkr.ecr.us-east-1.amazonaws.com/hebbs-platform:${{ github.sha }}
        docker tag hebbs-platform:${{ github.sha }} <account>.dkr.ecr.us-east-1.amazonaws.com/hebbs-platform:latest
        docker push <account>.dkr.ecr.us-east-1.amazonaws.com/hebbs-platform:${{ github.sha }}
        docker push <account>.dkr.ecr.us-east-1.amazonaws.com/hebbs-platform:latest

  deploy-us:
    needs: build-and-push
    runs-on: ubuntu-latest
    steps:
    - name: Update EKS deployment (US)
      run: |
        aws eks update-kubeconfig --name hebbs-central --region us-east-1
        kubectl set image deployment/platform-central \
          platform=<account>.dkr.ecr.us-east-1.amazonaws.com/hebbs-platform:${{ github.sha }} \
          -n hebbs-central
        kubectl set image deployment/platform-regional-us \
          platform=<account>.dkr.ecr.us-east-1.amazonaws.com/hebbs-platform:${{ github.sha }} \
          -n hebbs-us
        kubectl rollout status deployment/platform-central -n hebbs-central
        kubectl rollout status deployment/platform-regional-us -n hebbs-us

  deploy-eu:
    needs: build-and-push
    runs-on: ubuntu-latest
    steps:
    - name: Update EKS deployment (EU)
      run: |
        aws eks update-kubeconfig --name hebbs-eu --region eu-west-1
        kubectl set image deployment/platform-regional-eu \
          platform=<account>.dkr.ecr.eu-west-1.amazonaws.com/hebbs-platform:${{ github.sha }} \
          -n hebbs-eu
        kubectl rollout status deployment/platform-regional-eu -n hebbs-eu
```

### Engine updates (hebbs-server image)

Separate pipeline from the `hebbs` repo:

```yaml
# In hebbs/ repo — triggered on release tags
name: Build Engine Image
on:
  push:
    tags: ['v*']
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
    - name: Build hebbs-server image
      run: |
        docker build -t hebbs-server:${{ github.ref_name }} \
          --build-arg FEATURES="server,openai-embed,openai-llm" .
        # Push to ECR (both regions)
        # Rolling update of workspace containers (canary)
```

Workspace container updates are rolling — Kubernetes restarts each workspace pod one at a time. RocksDB recovers from WAL on restart. No data loss.

---

## Cost at scale

| Scale | Workspaces | Nodes (US) | Nodes (EU) | RDS | Total/mo |
|---|---|---|---|---|---|
| Launch | 10 | 2+2 | 2+2 | db.t4g.medium | ~$500 |
| 100 workspaces | 100 | 2+4 | 2+4 | db.t4g.medium | ~$1,200 |
| 500 workspaces | 500 | 2+16 | 2+16 | db.t4g.large | ~$4,000 |
| 1,000 workspaces | 1,000 | 2+32 | 2+32 | db.r6g.large | ~$8,000 |

Most workspaces are idle most of the time. Suspended free-tier workspaces (container stopped) cost only EBS storage (~$0.08/GB/mo = ~$0.10/workspace/mo).

At 100 Pro workspaces ($49/mo each): $4,900 revenue vs ~$1,200 infra = **~75% margin**.

---

## Day-one checklist

```
[ ] Route 53: subdomains created
[ ] ACM: TLS certs issued + validated (US + EU)
[ ] ECR: repos created, images pushed
[ ] EKS US: cluster + node groups + EBS CSI + storage class
[ ] EKS EU: cluster + node groups + EBS CSI + storage class
[ ] RDS: Postgres instance running, schema migrated
[ ] Secrets: OpenAI key, Stripe key, DB password in Secrets Manager + k8s
[ ] Platform central: deployed, healthy
[ ] Platform regional US: deployed, healthy
[ ] Platform regional EU: deployed, healthy
[ ] ALB US: ingress created, api.hebbs.ai + us.api.hebbs.ai resolving
[ ] ALB EU: ingress created, eu.api.hebbs.ai resolving
[ ] Console: SPA built, uploaded to S3, CloudFront serving app.hebbs.ai
[ ] DNS: all records pointing to correct targets
[ ] Monitoring: CloudWatch / Prometheus configured, alerts set
[ ] CI/CD: GitHub Actions deploying on push to main
[ ] Smoke test: sign up → create workspace → push docs → recall → works
```

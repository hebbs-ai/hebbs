# Technology Stack Decisions

## Programming Languages

Python 3.11 is the primary backend language, chosen for its ecosystem maturity and ML integration capabilities. TypeScript 5.3 powers all frontend applications. Go is used for performance-critical infrastructure tooling (log aggregation, metrics proxy).

## Database Technology

PostgreSQL 16 is the primary relational database, selected for its JSONB support, advanced indexing, and strong ACID guarantees. We run on AWS RDS with Multi-AZ deployment for high availability. Read replicas handle analytics queries to avoid impacting transactional workloads.

Redis 7 serves as the distributed cache and session store. We use Redis Cluster mode with 6 nodes for horizontal scaling.

## Infrastructure Platform

All services run on Amazon EKS (Kubernetes 1.28) across us-east-1, us-west-2, and eu-west-1 regions. Infrastructure provisioning uses Terraform 1.6 with modules stored in a private registry. Container images are built with multi-stage Dockerfiles and scanned by Trivy before deployment.

## Observability Stack

Datadog provides unified monitoring with APM traces, infrastructure metrics, and log management. Custom dashboards track SLOs for each service. PagerDuty handles alerting with escalation policies per team. We maintain 99.95% uptime SLO across all customer-facing services.

## Message Queue

Apache Kafka 3.6 handles event streaming with 12 brokers across 3 availability zones. Schema Registry enforces Avro schemas for all topics. Consumer groups are managed per team with dedicated quotas.

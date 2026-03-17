# Project Decisions

## Architecture

We decided to use microservices architecture for the backend. Each service communicates via gRPC. The API gateway handles authentication and rate limiting.

## Database

PostgreSQL is the primary database for transactional data. Redis is used for caching and session management. We chose TimescaleDB for time-series metrics.

## Budget

The infrastructure budget is $5,000 per tenant per month. This covers compute, storage, and network costs across all environments.

## Deployment

All services are deployed on AWS EKS (Kubernetes). CI/CD runs through GitHub Actions. Staging deploys happen automatically on PR merge to develop branch. Production deploys require manual approval.

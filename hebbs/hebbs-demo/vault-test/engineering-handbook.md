# Engineering Handbook

## Code Review

All pull requests require at least one approval before merging. The author is responsible for getting timely reviews — if a PR sits for more than 2 business days, ping the reviewer directly in Slack. Reviewers should complete reviews within 1 business day of being assigned.

PRs should be small and focused. A good PR addresses one concern and can be reviewed in under 20 minutes. Large PRs must include a summary comment explaining the overall design before reviewers start.

Never approve your own PR. Force pushes to main are blocked. Squash merges are the default — keep commit history clean.

## On-Call

On-call rotation is weekly, Monday to Monday. The primary on-call owns all P0 and P1 incidents during their week. The secondary is backup and helps with escalations. Both carry a pager.

Response SLA: P0 incidents require acknowledgement within 5 minutes and resolution within 30 minutes. P1 incidents require acknowledgement within 15 minutes.

After every P0, a blameless postmortem is required within 48 hours. The postmortem template is in Notion under Engineering > Postmortems.

Current on-call schedule is managed in PagerDuty. Carol Singh is the rotation coordinator.

## Deployments

Production deploys happen on Tuesdays and Thursdays unless urgent. The deploying engineer monitors the deploy for 30 minutes post-rollout and checks error rates and latency in Grafana.

Rollback is one command: `kubectl rollout undo deployment/<name>`. The deploying engineer owns the rollback decision — do not wait for manager approval during an incident.

Staging deploys happen automatically on every merge to the develop branch via GitHub Actions. Staging is not production — it is OK to break staging temporarily.

## Testing

Every feature PR must include tests. Unit test coverage must not drop below 80% for changed files. Integration tests run in CI against a real PostgreSQL instance — no mocking the database.

Performance-sensitive code paths (search, embedding, API endpoints) require a benchmark. Benchmarks live in `benches/` and are tracked over time in our internal dashboard.

## Security

Never commit secrets. Use environment variables or AWS Secrets Manager. The `.env` file is gitignored but must never be committed, even to private repos.

All customer data must be encrypted at rest (AES-256) and in transit (TLS 1.3). Access to production databases requires MFA and is logged.

Security incidents go to security@company.com and the engineering manager immediately. Do not attempt to contain a security incident without looping in the security team.

## Tooling

- Language: Go (backend), TypeScript/React (frontend), Python (data/ML)
- Infrastructure: AWS EKS, RDS PostgreSQL, ElastiCache Redis, S3, CloudFront
- Monitoring: Grafana, Prometheus, PagerDuty
- CI/CD: GitHub Actions
- Secrets: AWS Secrets Manager
- Comms: Slack (#engineering, #incidents, #deploys)

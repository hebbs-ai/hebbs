# Product Roadmap 2024

## Q1 (Jan–Mar)

### Search v2 (P0)
Mobile search performance is the top priority. Current p95 latency on mobile is 4.2s — target is under 800ms. Alice owns the backend; Bob owns the frontend. Ship date: March 31.

Key work:
- Switch from full-text to vector search for query understanding
- Add Redis caching layer for top-1000 queries
- Compress search index for mobile network conditions
- CDN pre-warming for predictable query patterns

### Dark Mode (P1)
Three enterprise customers have formally requested dark mode as a contract requirement. Bob owns implementation. Target: April 15 (slipped from March due to search priority).

## Q2 (Apr–Jun)

### Multi-tenant Dashboard
Each customer gets an isolated analytics dashboard showing their own usage, search quality, and API consumption. David owns requirements; Alice owns API; Bob owns frontend.

Key decisions outstanding:
- Row-level security in PostgreSQL vs separate schemas per tenant
- Real-time updates via WebSocket or polling
- Data retention policy (default: 90 days, configurable up to 2 years)

### API v2
Breaking changes required for consistency. v1 API will be supported for 12 months post-v2 launch. Migration guide is David's responsibility to write.

Breaking changes planned:
- Rename `user_id` to `tenant_user_id` across all endpoints
- Switch pagination from offset to cursor-based
- Remove deprecated `/search/legacy` endpoint

### Recommendation Engine
Eve's team will ship the first version of personalized recommendations. Uses collaborative filtering on search history. Requires opt-in from tenants due to data usage.

## Q3 (Jul–Sep)

### Mobile App
Native iOS and Android apps. Scope TBD pending Q2 learnings. David to finalize requirements by June 1.

### SOC 2 Type II
Compliance audit scheduled for Q3. Carol owns the technical controls. Legal owns the policy documentation. All engineers will need to complete security training by July 1.

## Q4 (Oct–Dec)

### Enterprise SSO
SAML and OIDC support for enterprise customers. Required for 3 deals currently in procurement.

### Offline Mode
Allow mobile users to search a local cache when offline. Technical feasibility study due Q2.

## Metrics

- MAU target 2024: 500K (currently 280K)
- Search p95 latency target: <800ms (currently 4.2s on mobile)
- Net Promoter Score target: 45 (currently 31)
- Uptime SLA: 99.9% (achieved 99.94% in 2023)

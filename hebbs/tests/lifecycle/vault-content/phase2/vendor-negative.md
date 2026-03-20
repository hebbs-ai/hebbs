# DataPipe Vendor Assessment: Critical Issues

## Reliability Problems

DataPipe has been an unreliable data integration platform. Over the past 12 months, we experienced 14 unplanned outages totaling 47 hours of downtime. Their claimed 99.95% SLA is not being met; actual availability was 99.46%. The failover mechanism failed to activate during 3 separate incidents, requiring manual intervention.

## Throughput Limitations

The platform struggles with our load, maxing out at 2,000 events per second before queueing delays exceed acceptable thresholds. During Black Friday 2025, the system buckled under load and we lost approximately 8% of events. Batch processing jobs frequently timeout and require manual restarts.

## Cost Overruns

DataPipe has become our most expensive data integration tool, with costs escalating 60% year-over-year. Hidden charges for premium connectors and overage fees brought the actual annual cost to $420,000, far exceeding the original $250,000 estimate. The consumption-based pricing model penalizes our bursty traffic patterns.

## Support Failures

Their technical support team regularly misses SLA response times. P1 issues have waited up to 18 hours for initial response. The assigned account engineer was reassigned twice in six months with no transition documentation. Escalations to management have not improved the situation.

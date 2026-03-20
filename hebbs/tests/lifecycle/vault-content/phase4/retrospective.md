# Q2 Engineering Retrospective

## Root Cause Analysis

The velocity decline from 30 to 14 story points over five sprints traces to three compounding factors: key person dependencies, insufficient documentation, and unsustainable on-call load.

The departure of the senior Kafka engineer exposed critical knowledge silos. Undocumented configuration dependencies and tribal knowledge created a 3-week recovery period where the team operated at reduced capacity while reverse-engineering systems.

## Systemic Issues

On-call burden increased 300% after the first departure, as fewer engineers rotated through the same incident volume. This created a feedback loop: burnout from on-call led to the second resignation, further concentrating the load.

## Recommended Actions

Immediate: hire two senior backend engineers with distributed systems experience. Implement mandatory documentation requirements for all system-critical components. Redistribute on-call to include frontend engineers for user-facing incidents.

Medium-term: invest in chaos engineering to identify single points of failure before they cause production incidents. Implement automated runbooks for the top 10 most common incident types. Establish a technical writing practice to eliminate knowledge silos.

## Metrics Summary

Sprint velocity trend: 30, 26, 22, 18, 14 story points. Sprint completion rate trend: 94%, 87%, 79%, 69%, 58%. Engineer attrition: 2 senior engineers (40% of senior backend capacity). Production incidents: 7 major incidents in Q2 vs 0 in Q1.

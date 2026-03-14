## Overview

Project Alpha is a six-month initiative to build a unified data platform for the analytics team. The platform consolidates three legacy systems into a single query interface, reducing operational overhead and enabling cross-dataset joins that were previously impossible.

The project depends on the infrastructure work being done in [[note_linked_b]], which provides the shared storage layer that Alpha will build on top of.

## Timeline

Phase 1 (January through February) focused on schema migration and data validation. All 14 source tables were mapped to the new unified schema, and a parallel-write pipeline was deployed to populate the new store without disrupting existing consumers.

Phase 2 (March through April) will deliver the query API and deprecate direct database access. The team is targeting feature parity with the legacy systems by the end of March, with a two-week buffer for edge cases discovered during integration testing.

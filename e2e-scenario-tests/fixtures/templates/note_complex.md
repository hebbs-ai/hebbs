---
title: "System Architecture for Event-Driven Memory Indexing"
author: "Jasen Harpe"
date: 2026-03-10
hebbs-importance: 0.8
---

## Introduction

This document describes the architecture for event-driven memory indexing in large-scale knowledge systems. The goal is to replace the current poll-based sync with a reactive pipeline that processes changes within milliseconds of detection. See [[other-note]] for the original proposal that motivated this work.

Event-driven architectures offer several advantages over traditional batch processing. They reduce latency, improve resource utilization, and simplify the programming model for downstream consumers. In our case, the primary consumer is the retrieval engine, which must always serve the freshest possible index. #architecture

## Background

The existing system uses a 30-second polling loop to detect file changes. This introduces unnecessary latency and wastes CPU cycles scanning unchanged directories. Profiling showed that 94% of poll cycles find zero changes, yet each cycle still performs a full directory walk.

Previous attempts to use filesystem watchers (inotify on Linux, FSEvents on macOS) were abandoned due to platform inconsistencies and event coalescing issues. The new design addresses these concerns by introducing a unified change detection layer that normalizes platform-specific events into a common stream. See [[api-design#endpoints]] for the change event schema. #design/patterns

## Design

The architecture consists of four components: the change detector, the event bus, the indexing pipeline, and the consistency checker.

The change detector monitors the filesystem and emits normalized change events. Each event includes the file path, change type (created, modified, deleted), content hash, and a monotonic sequence number. The sequence number is critical for ordering guarantees when multiple changes arrive within the same millisecond.

The event bus is a bounded, in-process channel that decouples detection from indexing. It applies backpressure when the indexing pipeline falls behind, preventing unbounded memory growth. The bus supports multiple subscribers, allowing the consistency checker to observe the same event stream without duplicating detection work. #architecture #design/patterns

## Implementation

The indexing pipeline consumes events from the bus and updates the memory index. Each event triggers a targeted re-index of the affected memory rather than a full rebuild. For modifications, the pipeline computes a diff against the previous version and updates only the changed sections.

Deletions are handled through a soft-delete mechanism. The memory is marked as tombstoned with a TTL, allowing the retrieval engine to filter it from results immediately while the storage layer reclaims space during compaction. This avoids blocking the hot path with expensive delete operations.

The pipeline maintains a write-ahead log for crash recovery. If the process terminates between receiving an event and completing the index update, the WAL replays uncommitted events on restart. The WAL is bounded to 1000 entries and is truncated after each successful checkpoint.

## Conclusion

The event-driven architecture reduces indexing latency from 30 seconds (poll interval) to under 50 milliseconds (p99). It eliminates wasted CPU cycles from no-op polls and provides stronger consistency guarantees through the sequence-numbered event stream.

The next phase will extend the event bus to support distributed deployments, where multiple engine instances share a partitioned event log. This requires careful handling of partition rebalancing and exactly-once delivery semantics.

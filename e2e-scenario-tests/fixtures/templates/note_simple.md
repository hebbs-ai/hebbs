## Weekly Status Update

The team shipped the new ingestion pipeline on Tuesday. Throughput improved by 40% compared to the previous batch-oriented approach, and error rates dropped below the 0.1% threshold we set at the start of the quarter.

Next week we plan to focus on the deduplication layer. Several customers reported seeing near-duplicate entries when the same document is ingested through both the API and the file watcher. The fix likely involves normalizing content hashes before the uniqueness check.

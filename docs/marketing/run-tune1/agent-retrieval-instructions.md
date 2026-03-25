# Hebbs Retrieval Instructions for Agents

Use these rules when calling `hebbs recall`. Classify each query, then apply the matching strategy.

## Strategy Selection

| Query pattern | Strategy | Flags |
|---|---|---|
| Simple fact lookup ("What is X?", "Who is Y?") | similarity | `-k 5` |
| Detailed fact with context ("What happened during X?") | similarity | `-k 10`, include entity names in cue |
| Evolution/timeline ("How has X changed?", "What changed since?") | similarity | `-k 10 --weights 0.3:0.5:0.2:0`, include dates and entity names in cue |
| Cross-entity patterns ("Which vendors?", "Compare across?") | analogical | `-k 10 --strategy analogical --analogical-alpha 0.3` |
| Causal chain ("Why did X happen?", "What caused Y?") | causal | `--strategy causal --seed <ID> --max-depth 3 --edge-types caused_by` |
| Temporal entity history | temporal | `--strategy temporal --entity-id <entity> -k 10` |

## Scoring Weights

Format: `--weights relevance:recency:importance:reinforcement` (must sum to ~1.0).

| Goal | Weights |
|---|---|
| Default (balanced) | `1:0:0:0` (similarity only) |
| Recency-biased | `0.3:0.5:0.2:0` |
| Importance-biased | `0.3:0.3:0.4:0` |
| Reinforcement-biased | `0.3:0.2:0.2:0.3` |

## Key Rules

1. **Default to k=10.** k=5 misses supporting details on most queries. Use k=5 only for simple factual lookups.
2. **Include entity names in cues.** "Meridian risk" beats "vendor risk" for specific queries.
3. **Include dates/timeframes.** "Q4 2024 to Q2 2025" beats "recent changes".
4. **Use analogical for cross-entity.** Any query comparing multiple entities or finding patterns across them.
5. **Causal needs setup.** Store entity-scoped memories with `--entity-id` and `--edge TARGET:caused_by:CONFIDENCE` first. Then recall with `--seed`.
6. **Temporal needs entity_id.** Only works on memories stored with `--entity-id`. For indexed content, use recency-weighted similarity instead.

## Storing Learned Strategies

When you discover a retrieval approach that works, store it:

```
hebbs remember "STRATEGY: For [query pattern], use [flags]" --importance 0.9
```

## Building Entity Timelines

To enable temporal/causal recall for an entity, store key facts with edges:

```
hebbs remember "EVENT 1" --entity-id <entity> --importance 0.8
hebbs remember "EVENT 2" --entity-id <entity> --edge <EVENT1_ID>:followed_by:0.9
hebbs remember "CONSEQUENCE" --entity-id <entity> --edge <EVENT2_ID>:caused_by:0.9
```

Then recall the chain:
```
hebbs recall "query" --strategy temporal --entity-id <entity>
hebbs recall "query" --strategy causal --seed <CONSEQUENCE_ID> --edge-types caused_by
```

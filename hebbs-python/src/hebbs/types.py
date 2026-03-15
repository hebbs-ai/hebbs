"""Public data types for the HEBBS Python SDK.

All types are plain dataclasses -- no protobuf leakage in the public API.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Any


class MemoryKind(Enum):
    EPISODE = "episode"
    INSIGHT = "insight"
    REVISION = "revision"
    UNSPECIFIED = "unspecified"


class EdgeType(Enum):
    CAUSED_BY = "caused_by"
    RELATED_TO = "related_to"
    FOLLOWED_BY = "followed_by"
    REVISED_FROM = "revised_from"
    INSIGHT_FROM = "insight_from"
    CONTRADICTS = "contradicts"
    UNSPECIFIED = "unspecified"


class RecallStrategy(Enum):
    SIMILARITY = "similarity"
    TEMPORAL = "temporal"
    CAUSAL = "causal"
    ANALOGICAL = "analogical"


@dataclass(frozen=True)
class Edge:
    target_id: bytes
    edge_type: EdgeType
    confidence: float | None = None


@dataclass
class Memory:
    id: bytes
    content: str
    importance: float
    context: dict[str, Any]
    entity_id: str | None = None
    created_at: int = 0
    updated_at: int = 0
    last_accessed_at: int = 0
    access_count: int = 0
    decay_score: float = 0.0
    kind: MemoryKind = MemoryKind.EPISODE
    embedding: list[float] = field(default_factory=list)
    source_memory_ids: list[bytes] = field(default_factory=list)


@dataclass
class StrategyDetail:
    strategy: str
    relevance: float = 0.0
    distance: float | None = None
    timestamp: int | None = None
    rank: int | None = None
    depth: int | None = None
    embedding_similarity: float | None = None
    structural_similarity: float | None = None


@dataclass
class RecallResult:
    memory: Memory
    score: float
    strategy_details: list[StrategyDetail] = field(default_factory=list)


@dataclass
class StrategyError:
    strategy: str
    message: str


@dataclass
class RecallOutput:
    results: list[RecallResult]
    strategy_errors: list[StrategyError] = field(default_factory=list)


@dataclass
class PrimeOutput:
    results: list[RecallResult]
    temporal_count: int = 0
    similarity_count: int = 0


@dataclass
class RecallStrategyConfig:
    """Per-strategy configuration for advanced recall tuning.

    Most users should just pass strategy names as strings::

        strategies=["similarity", "temporal"]

    Use this class when you need to tune strategy-specific parameters::

        strategies=[RecallStrategyConfig("causal", seed_memory_id=mem.id, max_depth=3)]

    All fields except ``strategy`` are optional and use smart engine defaults.
    """
    strategy: str
    entity_id: str | None = None
    top_k: int | None = None
    ef_search: int | None = None
    time_range: tuple[int, int] | None = None
    seed_memory_id: bytes | None = None
    edge_types: list[EdgeType] | None = None
    max_depth: int | None = None
    analogical_alpha: float | None = None


@dataclass
class ScoringWeights:
    """Composite scoring weight overrides for recall and prime operations.

    When omitted (None), the engine uses default weights:
    w_relevance=0.5, w_recency=0.2, w_importance=0.2, w_reinforcement=0.1.
    """
    w_relevance: float = 0.5
    w_recency: float = 0.2
    w_importance: float = 0.2
    w_reinforcement: float = 0.1
    max_age_us: int | None = None
    reinforcement_cap: int | None = None


@dataclass
class ForgetResult:
    forgotten_count: int
    cascade_count: int
    tombstone_count: int
    truncated: bool = False


@dataclass
class ReflectResult:
    insights_created: int
    clusters_found: int
    clusters_processed: int
    memories_processed: int


@dataclass
class SubscribePush:
    subscription_id: int
    memory: Memory
    confidence: float
    push_timestamp_us: int = 0
    sequence_number: int = 0


@dataclass
class HealthStatus:
    serving: bool
    version: str
    memory_count: int
    uptime_seconds: int


@dataclass
class ClusterMemorySummary:
    memory_id: str
    content: str
    importance: float
    entity_id: str | None = None
    created_at: int = 0


@dataclass
class ClusterPrompt:
    cluster_id: int
    member_count: int
    proposal_system_prompt: str
    proposal_user_prompt: str
    memory_ids: list[str] = field(default_factory=list)
    validation_context: str = ""
    memories: list[ClusterMemorySummary] = field(default_factory=list)


@dataclass
class ReflectPrepareResult:
    session_id: str
    memories_processed: int
    clusters: list[ClusterPrompt] = field(default_factory=list)
    existing_insight_count: int = 0


@dataclass
class ProducedInsightInput:
    content: str
    confidence: float
    source_memory_ids: list[str] = field(default_factory=list)
    tags: list[str] = field(default_factory=list)
    cluster_id: int | None = None


@dataclass
class ReflectCommitResult:
    insights_created: int

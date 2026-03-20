"""Async wrapper for the HEBBS MemoryService gRPC methods."""

from __future__ import annotations

from typing import Any

from google.protobuf import struct_pb2

from hebbs._generated import hebbs_pb2, hebbs_pb2_grpc
from hebbs.exceptions import _map_grpc_error
from hebbs.types import (
    Edge,
    EdgeType,
    ForgetResult,
    Memory,
    MemoryKind,
    PrimeOutput,
    RecallOutput,
    RecallResult,
    RecallStrategy,
    RecallStrategyConfig,
    ScoringWeights,
    StrategyDetail,
    StrategyError,
)

_EDGE_TYPE_MAP = {
    EdgeType.CAUSED_BY: hebbs_pb2.EDGE_TYPE_CAUSED_BY,
    EdgeType.RELATED_TO: hebbs_pb2.EDGE_TYPE_RELATED_TO,
    EdgeType.FOLLOWED_BY: hebbs_pb2.EDGE_TYPE_FOLLOWED_BY,
    EdgeType.REVISED_FROM: hebbs_pb2.EDGE_TYPE_REVISED_FROM,
    EdgeType.INSIGHT_FROM: hebbs_pb2.EDGE_TYPE_INSIGHT_FROM,
    EdgeType.CONTRADICTS: hebbs_pb2.EDGE_TYPE_CONTRADICTS,
    EdgeType.HAS_ENTITY: hebbs_pb2.EDGE_TYPE_HAS_ENTITY,
    EdgeType.ENTITY_RELATION: hebbs_pb2.EDGE_TYPE_ENTITY_RELATION,
    EdgeType.PROPOSITION_OF: hebbs_pb2.EDGE_TYPE_PROPOSITION_OF,
}

_STRATEGY_MAP = {
    "similarity": hebbs_pb2.SIMILARITY,
    "temporal": hebbs_pb2.TEMPORAL,
    "causal": hebbs_pb2.CAUSAL,
    "analogical": hebbs_pb2.ANALOGICAL,
}

_MEMORY_KIND_MAP = {
    MemoryKind.EPISODE: hebbs_pb2.MEMORY_KIND_EPISODE,
    MemoryKind.INSIGHT: hebbs_pb2.MEMORY_KIND_INSIGHT,
    MemoryKind.REVISION: hebbs_pb2.MEMORY_KIND_REVISION,
    MemoryKind.DOCUMENT: hebbs_pb2.MEMORY_KIND_DOCUMENT,
    MemoryKind.PROPOSITION: hebbs_pb2.MEMORY_KIND_PROPOSITION,
}

_MEMORY_KIND_REVERSE: dict[int, MemoryKind] = {
    hebbs_pb2.MEMORY_KIND_EPISODE: MemoryKind.EPISODE,
    hebbs_pb2.MEMORY_KIND_INSIGHT: MemoryKind.INSIGHT,
    hebbs_pb2.MEMORY_KIND_REVISION: MemoryKind.REVISION,
    hebbs_pb2.MEMORY_KIND_DOCUMENT: MemoryKind.DOCUMENT,
    hebbs_pb2.MEMORY_KIND_PROPOSITION: MemoryKind.PROPOSITION,
}

_STRATEGY_REVERSE: dict[int, str] = {
    hebbs_pb2.SIMILARITY: "similarity",
    hebbs_pb2.TEMPORAL: "temporal",
    hebbs_pb2.CAUSAL: "causal",
    hebbs_pb2.ANALOGICAL: "analogical",
}


def _to_proto_scoring_weights(sw: ScoringWeights | dict) -> hebbs_pb2.ScoringWeights:
    if isinstance(sw, dict):
        sw = ScoringWeights(**{k: v for k, v in sw.items() if v is not None})
    proto = hebbs_pb2.ScoringWeights(
        w_relevance=sw.w_relevance,
        w_recency=sw.w_recency,
        w_importance=sw.w_importance,
        w_reinforcement=sw.w_reinforcement,
    )
    if sw.max_age_us is not None:
        proto.max_age_us = sw.max_age_us
    if sw.reinforcement_cap is not None:
        proto.reinforcement_cap = sw.reinforcement_cap
    return proto


def _dict_to_struct(d: dict[str, Any] | None) -> struct_pb2.Struct | None:
    if not d:
        return None
    s = struct_pb2.Struct()
    s.update(d)
    return s


def _struct_to_dict(s: Any) -> dict[str, Any]:
    if s is None or not s.fields:
        return {}
    from google.protobuf.json_format import MessageToDict
    return MessageToDict(s)


def _proto_to_memory(m: Any) -> Memory:
    return Memory(
        id=bytes(m.memory_id),
        content=m.content,
        importance=m.importance,
        context=_struct_to_dict(m.context),
        entity_id=m.entity_id if m.entity_id else None,
        created_at=m.created_at,
        updated_at=m.updated_at,
        last_accessed_at=m.last_accessed_at,
        access_count=m.access_count,
        decay_score=m.decay_score,
        kind=_MEMORY_KIND_REVERSE.get(m.kind, MemoryKind.UNSPECIFIED),
        embedding=list(m.embedding),
        source_memory_ids=[bytes(sid) for sid in m.source_memory_ids],
    )


def _proto_to_strategy_detail(sd: Any) -> StrategyDetail:
    return StrategyDetail(
        strategy=_STRATEGY_REVERSE.get(sd.strategy_type, "unknown"),
        relevance=sd.relevance,
        distance=sd.distance if sd.HasField("distance") else None,
        timestamp=sd.timestamp if sd.HasField("timestamp") else None,
        rank=sd.rank if sd.HasField("rank") else None,
        depth=sd.depth if sd.HasField("depth") else None,
        embedding_similarity=sd.embedding_similarity if sd.HasField("embedding_similarity") else None,
        structural_similarity=sd.structural_similarity if sd.HasField("structural_similarity") else None,
    )


def _proto_to_recall_result(r: Any) -> RecallResult:
    return RecallResult(
        memory=_proto_to_memory(r.memory),
        score=r.score,
        strategy_details=[_proto_to_strategy_detail(sd) for sd in r.strategy_details],
    )


def _build_strategy_config_proto(
    sc: RecallStrategyConfig,
    fallback_entity_id: str | None = None,
) -> hebbs_pb2.RecallStrategyConfig:
    st = _STRATEGY_MAP.get(sc.strategy, hebbs_pb2.UNSPECIFIED_STRATEGY)
    cfg = hebbs_pb2.RecallStrategyConfig(strategy_type=st)

    eid = sc.entity_id or fallback_entity_id
    if eid:
        cfg.entity_id = eid
    if sc.top_k is not None:
        cfg.top_k = sc.top_k
    if sc.ef_search is not None:
        cfg.ef_search = sc.ef_search
    if sc.time_range is not None:
        cfg.time_range.CopyFrom(
            hebbs_pb2.TimeRange(start_us=sc.time_range[0], end_us=sc.time_range[1])
        )
    if sc.seed_memory_id is not None:
        cfg.seed_memory_id = sc.seed_memory_id
    if sc.edge_types:
        for et in sc.edge_types:
            proto_et = _EDGE_TYPE_MAP.get(et, hebbs_pb2.EDGE_TYPE_UNSPECIFIED)
            cfg.edge_types.append(proto_et)
    if sc.max_depth is not None:
        cfg.max_depth = sc.max_depth
    if sc.analogical_alpha is not None:
        cfg.analogical_alpha = sc.analogical_alpha
    return cfg


class MemoryServiceClient:
    """Async client for the HEBBS MemoryService."""

    def __init__(self, stub: hebbs_pb2_grpc.MemoryServiceStub, tenant_id: str | None = None) -> None:
        self._stub = stub
        self._tenant_id = tenant_id

    async def remember(
        self,
        content: str,
        importance: float | None = None,
        context: dict[str, Any] | None = None,
        entity_id: str | None = None,
        edges: list[Edge] | None = None,
        kind: MemoryKind | None = None,
    ) -> Memory:
        req = hebbs_pb2.RememberRequest(content=content)
        if importance is not None:
            req.importance = importance
        ctx = _dict_to_struct(context)
        if ctx is not None:
            req.context.CopyFrom(ctx)
        if entity_id:
            req.entity_id = entity_id
        if self._tenant_id:
            req.tenant_id = self._tenant_id
        if kind is not None:
            req.kind = _MEMORY_KIND_MAP.get(kind, hebbs_pb2.MEMORY_KIND_UNSPECIFIED)
        if edges:
            for e in edges:
                proto_edge = hebbs_pb2.Edge(
                    target_id=e.target_id,
                    edge_type=_EDGE_TYPE_MAP.get(e.edge_type, hebbs_pb2.EDGE_TYPE_UNSPECIFIED),
                )
                if e.confidence is not None:
                    proto_edge.confidence = e.confidence
                req.edges.append(proto_edge)

        try:
            resp = await self._stub.Remember(req)
        except Exception as e:
            raise _map_grpc_error(e) from e
        return _proto_to_memory(resp.memory)

    async def get(self, memory_id: bytes) -> Memory:
        req = hebbs_pb2.GetRequest(memory_id=memory_id)
        if self._tenant_id:
            req.tenant_id = self._tenant_id
        try:
            resp = await self._stub.Get(req)
        except Exception as e:
            raise _map_grpc_error(e) from e
        return _proto_to_memory(resp.memory)

    async def recall(
        self,
        cue: str,
        strategies: list[str | RecallStrategyConfig] | None = None,
        top_k: int | None = None,
        entity_id: str | None = None,
        scoring_weights: ScoringWeights | dict | None = None,
        cue_context: dict[str, Any] | None = None,
    ) -> RecallOutput:
        strat_configs = []
        for s in (strategies or ["similarity"]):
            if isinstance(s, str):
                st = _STRATEGY_MAP.get(s, hebbs_pb2.UNSPECIFIED_STRATEGY)
                cfg = hebbs_pb2.RecallStrategyConfig(strategy_type=st)
                if entity_id:
                    cfg.entity_id = entity_id
            elif isinstance(s, RecallStrategyConfig):
                cfg = _build_strategy_config_proto(s, entity_id)
            else:
                raise TypeError(f"strategies must contain str or RecallStrategyConfig, got {type(s)}")
            strat_configs.append(cfg)

        req = hebbs_pb2.RecallRequest(cue=cue, strategies=strat_configs)
        if top_k is not None:
            req.top_k = top_k
        if self._tenant_id:
            req.tenant_id = self._tenant_id
        if scoring_weights is not None:
            req.scoring_weights.CopyFrom(_to_proto_scoring_weights(scoring_weights))
        if cue_context is not None:
            ctx = _dict_to_struct(cue_context)
            if ctx is not None:
                req.cue_context.CopyFrom(ctx)

        try:
            resp = await self._stub.Recall(req)
        except Exception as e:
            raise _map_grpc_error(e) from e

        return RecallOutput(
            results=[_proto_to_recall_result(r) for r in resp.results],
            strategy_errors=[
                StrategyError(
                    strategy=_STRATEGY_REVERSE.get(se.strategy, "unknown"),
                    message=se.message,
                )
                for se in resp.strategy_errors
            ],
        )

    async def prime(
        self,
        entity_id: str,
        max_memories: int | None = None,
        similarity_cue: str | None = None,
        scoring_weights: ScoringWeights | dict | None = None,
    ) -> PrimeOutput:
        req = hebbs_pb2.PrimeRequest(entity_id=entity_id)
        if max_memories is not None:
            req.max_memories = max_memories
        if similarity_cue:
            req.similarity_cue = similarity_cue
        if self._tenant_id:
            req.tenant_id = self._tenant_id
        if scoring_weights is not None:
            req.scoring_weights.CopyFrom(_to_proto_scoring_weights(scoring_weights))

        try:
            resp = await self._stub.Prime(req)
        except Exception as e:
            raise _map_grpc_error(e) from e

        return PrimeOutput(
            results=[_proto_to_recall_result(r) for r in resp.results],
            temporal_count=resp.temporal_count,
            similarity_count=resp.similarity_count,
        )

    async def revise(
        self,
        memory_id: bytes,
        content: str | None = None,
        importance: float | None = None,
        context: dict[str, Any] | None = None,
        entity_id: str | None = None,
    ) -> Memory:
        req = hebbs_pb2.ReviseRequest(memory_id=memory_id)
        if content is not None:
            req.content = content
        if importance is not None:
            req.importance = importance
        ctx = _dict_to_struct(context)
        if ctx is not None:
            req.context.CopyFrom(ctx)
        if entity_id:
            req.entity_id = entity_id
        if self._tenant_id:
            req.tenant_id = self._tenant_id

        try:
            resp = await self._stub.Revise(req)
        except Exception as e:
            raise _map_grpc_error(e) from e
        return _proto_to_memory(resp.memory)

    async def forget(
        self,
        entity_id: str | None = None,
        memory_ids: list[bytes] | None = None,
    ) -> ForgetResult:
        req = hebbs_pb2.ForgetRequest()
        if entity_id:
            req.entity_id = entity_id
        if memory_ids:
            req.memory_ids.extend(memory_ids)
        if self._tenant_id:
            req.tenant_id = self._tenant_id

        try:
            resp = await self._stub.Forget(req)
        except Exception as e:
            raise _map_grpc_error(e) from e

        return ForgetResult(
            forgotten_count=resp.forgotten_count,
            cascade_count=resp.cascade_count,
            tombstone_count=resp.tombstone_count,
            truncated=resp.truncated,
        )

    async def set_policy(
        self,
        max_snapshots_per_memory: int | None = None,
        auto_forget_threshold: float | None = None,
        decay_half_life_days: float | None = None,
    ) -> bool:
        req = hebbs_pb2.SetPolicyRequest()
        if self._tenant_id:
            req.tenant_id = self._tenant_id
        if max_snapshots_per_memory is not None:
            req.max_snapshots_per_memory = max_snapshots_per_memory
        if auto_forget_threshold is not None:
            req.auto_forget_threshold = auto_forget_threshold
        if decay_half_life_days is not None:
            req.decay_half_life_days = decay_half_life_days

        try:
            resp = await self._stub.SetPolicy(req)
        except Exception as e:
            raise _map_grpc_error(e) from e
        return resp.applied

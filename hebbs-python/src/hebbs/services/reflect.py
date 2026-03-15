"""Async wrapper for the HEBBS ReflectService gRPC methods."""

from __future__ import annotations

from hebbs._generated import hebbs_pb2, hebbs_pb2_grpc
from hebbs.exceptions import _map_grpc_error
from hebbs.services.memory import _proto_to_memory
from hebbs.types import (
    ClusterMemorySummary,
    ClusterPrompt,
    Memory,
    ProducedInsightInput,
    ReflectCommitResult,
    ReflectPrepareResult,
    ReflectResult,
)


class ReflectServiceClient:
    """Async client for the HEBBS ReflectService."""

    def __init__(self, stub: hebbs_pb2_grpc.ReflectServiceStub, tenant_id: str | None = None) -> None:
        self._stub = stub
        self._tenant_id = tenant_id

    async def reflect(self, entity_id: str | None = None) -> ReflectResult:
        scope = hebbs_pb2.ReflectScope()
        if entity_id:
            scope.entity.CopyFrom(hebbs_pb2.EntityScope(entity_id=entity_id))
        else:
            getattr(scope, "global").CopyFrom(hebbs_pb2.GlobalScope())

        req = hebbs_pb2.ReflectRequest(scope=scope)
        if self._tenant_id:
            req.tenant_id = self._tenant_id

        try:
            resp = await self._stub.Reflect(req)
        except Exception as e:
            raise _map_grpc_error(e) from e

        return ReflectResult(
            insights_created=resp.insights_created,
            clusters_found=resp.clusters_found,
            clusters_processed=resp.clusters_processed,
            memories_processed=resp.memories_processed,
        )

    async def get_insights(
        self,
        entity_id: str | None = None,
        max_results: int | None = None,
    ) -> list[Memory]:
        req = hebbs_pb2.GetInsightsRequest()
        if entity_id:
            req.entity_id = entity_id
        if max_results is not None:
            req.max_results = max_results
        if self._tenant_id:
            req.tenant_id = self._tenant_id

        try:
            resp = await self._stub.GetInsights(req)
        except Exception as e:
            raise _map_grpc_error(e) from e

        return [_proto_to_memory(m) for m in resp.insights]

    async def reflect_prepare(self, entity_id: str | None = None) -> ReflectPrepareResult:
        scope = hebbs_pb2.ReflectScope()
        if entity_id:
            scope.entity.CopyFrom(hebbs_pb2.EntityScope(entity_id=entity_id))
        else:
            getattr(scope, "global").CopyFrom(hebbs_pb2.GlobalScope())

        req = hebbs_pb2.ReflectPrepareRequest(scope=scope)
        if self._tenant_id:
            req.tenant_id = self._tenant_id

        try:
            resp = await self._stub.ReflectPrepare(req)
        except Exception as e:
            raise _map_grpc_error(e) from e

        clusters = []
        for c in resp.clusters:
            memories = [
                ClusterMemorySummary(
                    memory_id=m.memory_id,
                    content=m.content,
                    importance=m.importance,
                    entity_id=m.entity_id or None,
                    created_at=m.created_at,
                )
                for m in c.memories
            ]
            clusters.append(ClusterPrompt(
                cluster_id=c.cluster_id,
                member_count=c.member_count,
                proposal_system_prompt=c.proposal_system_prompt,
                proposal_user_prompt=c.proposal_user_prompt,
                memory_ids=list(c.memory_ids),
                validation_context=c.validation_context,
                memories=memories,
            ))

        return ReflectPrepareResult(
            session_id=resp.session_id,
            memories_processed=resp.memories_processed,
            clusters=clusters,
            existing_insight_count=resp.existing_insight_count,
        )

    async def reflect_commit(
        self,
        session_id: str,
        insights: list[ProducedInsightInput],
    ) -> ReflectCommitResult:
        proto_insights = []
        for ins in insights:
            proto_ins = hebbs_pb2.ProducedInsightInput(
                content=ins.content,
                confidence=ins.confidence,
                source_memory_ids=ins.source_memory_ids,
                tags=ins.tags,
            )
            if ins.cluster_id is not None:
                proto_ins.cluster_id = ins.cluster_id
            proto_insights.append(proto_ins)

        req = hebbs_pb2.ReflectCommitRequest(
            session_id=session_id,
            insights=proto_insights,
        )
        if self._tenant_id:
            req.tenant_id = self._tenant_id

        try:
            resp = await self._stub.ReflectCommit(req)
        except Exception as e:
            raise _map_grpc_error(e) from e

        return ReflectCommitResult(insights_created=resp.insights_created)

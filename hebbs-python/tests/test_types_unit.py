"""Unit tests for the HEBBS Python SDK types."""

from hebbs import (
    EdgeType,
    MemoryKind,
    RecallStrategy,
    Memory,
    Edge,
    RecallResult,
    RecallOutput,
    PrimeOutput,
    ScoringWeights,
    ForgetResult,
    ReflectResult,
    SubscribePush,
    HealthStatus,
    RecallStrategyConfig,
    StrategyDetail,
    StrategyError,
    ClusterMemorySummary,
    ClusterPrompt,
    ProducedInsightInput,
    ReflectCommitResult,
    ReflectPrepareResult,
)


class TestEdgeType:
    def test_all_values(self):
        assert EdgeType.CAUSED_BY.value == "caused_by"
        assert EdgeType.RELATED_TO.value == "related_to"
        assert EdgeType.FOLLOWED_BY.value == "followed_by"
        assert EdgeType.REVISED_FROM.value == "revised_from"
        assert EdgeType.INSIGHT_FROM.value == "insight_from"
        assert EdgeType.CONTRADICTS.value == "contradicts"
        assert EdgeType.UNSPECIFIED.value == "unspecified"

    def test_contradicts_is_present(self):
        """EDGE_TYPE_CONTRADICTS was added for contradiction detection."""
        assert hasattr(EdgeType, "CONTRADICTS")
        assert EdgeType.CONTRADICTS.value == "contradicts"


class TestMemoryKind:
    def test_all_values(self):
        assert MemoryKind.EPISODE.value == "episode"
        assert MemoryKind.INSIGHT.value == "insight"
        assert MemoryKind.REVISION.value == "revision"
        assert MemoryKind.UNSPECIFIED.value == "unspecified"


class TestReflectPrepareResult:
    def test_construction(self):
        result = ReflectPrepareResult(
            session_id="sess-123",
            memories_processed=42,
            clusters=[
                ClusterPrompt(
                    cluster_id=0,
                    member_count=5,
                    proposal_system_prompt="You are...",
                    proposal_user_prompt="Analyze...",
                    memory_ids=["m1", "m2"],
                    validation_context="context",
                    memories=[
                        ClusterMemorySummary(
                            memory_id="m1",
                            content="Test memory",
                            importance=0.8,
                            entity_id="ent-1",
                            created_at=1000,
                        )
                    ],
                )
            ],
            existing_insight_count=3,
        )
        assert result.session_id == "sess-123"
        assert result.memories_processed == 42
        assert len(result.clusters) == 1
        assert result.clusters[0].member_count == 5
        assert len(result.clusters[0].memories) == 1
        assert result.clusters[0].memories[0].content == "Test memory"
        assert result.existing_insight_count == 3

    def test_defaults(self):
        result = ReflectPrepareResult(session_id="s", memories_processed=0)
        assert result.clusters == []
        assert result.existing_insight_count == 0


class TestProducedInsightInput:
    def test_construction(self):
        insight = ProducedInsightInput(
            content="Users prefer API-first",
            confidence=0.85,
            source_memory_ids=["m1", "m2", "m3"],
            tags=["preference", "api"],
            cluster_id=2,
        )
        assert insight.content == "Users prefer API-first"
        assert insight.confidence == 0.85
        assert len(insight.source_memory_ids) == 3
        assert insight.cluster_id == 2

    def test_defaults(self):
        insight = ProducedInsightInput(content="test", confidence=0.5)
        assert insight.source_memory_ids == []
        assert insight.tags == []
        assert insight.cluster_id is None


class TestReflectCommitResult:
    def test_construction(self):
        result = ReflectCommitResult(insights_created=5)
        assert result.insights_created == 5


class TestPendingContradiction:
    def test_construction(self):
        from hebbs import PendingContradiction

        pc = PendingContradiction(
            pending_id="abc123",
            memory_id_a="mem_a",
            memory_id_b="mem_b",
            content_a_snippet="The system is reliable",
            content_b_snippet="The system is unreliable",
            classifier_score=0.65,
            classifier_method="heuristic",
            similarity=0.82,
            created_at=1_700_000_000_000_000,
        )
        assert pc.pending_id == "abc123"
        assert pc.memory_id_a == "mem_a"
        assert pc.memory_id_b == "mem_b"
        assert pc.content_a_snippet == "The system is reliable"
        assert pc.content_b_snippet == "The system is unreliable"
        assert pc.classifier_score == 0.65
        assert pc.classifier_method == "heuristic"
        assert pc.similarity == 0.82
        assert pc.created_at == 1_700_000_000_000_000

    def test_defaults(self):
        from hebbs import PendingContradiction

        pc = PendingContradiction(
            pending_id="id",
            memory_id_a="a",
            memory_id_b="b",
            content_a_snippet="x",
            content_b_snippet="y",
            classifier_score=0.5,
            classifier_method="heuristic",
            similarity=0.7,
        )
        assert pc.created_at == 0


class TestContradictionVerdictInput:
    def test_construction(self):
        from hebbs import ContradictionVerdictInput

        v = ContradictionVerdictInput(
            pending_id="abc123",
            verdict="contradiction",
            confidence=0.9,
            reasoning="Direct conflict in vendor assessment",
        )
        assert v.pending_id == "abc123"
        assert v.verdict == "contradiction"
        assert v.confidence == 0.9
        assert v.reasoning == "Direct conflict in vendor assessment"

    def test_dismiss_verdict(self):
        from hebbs import ContradictionVerdictInput

        v = ContradictionVerdictInput(
            pending_id="def456",
            verdict="dismiss",
            confidence=0.95,
        )
        assert v.verdict == "dismiss"
        assert v.reasoning is None

    def test_revision_verdict(self):
        from hebbs import ContradictionVerdictInput

        v = ContradictionVerdictInput(
            pending_id="ghi789",
            verdict="revision",
            confidence=0.85,
            reasoning="Updated timeline",
        )
        assert v.verdict == "revision"


class TestContradictionCommitResult:
    def test_construction(self):
        from hebbs import ContradictionCommitResult

        r = ContradictionCommitResult(
            contradictions_confirmed=2,
            revisions_created=1,
            dismissed=3,
        )
        assert r.contradictions_confirmed == 2
        assert r.revisions_created == 1
        assert r.dismissed == 3


class TestEdgeWithContradicts:
    def test_edge_with_contradicts(self):
        edge = Edge(
            target_id=b"\x01" * 16,
            edge_type=EdgeType.CONTRADICTS,
            confidence=0.75,
        )
        assert edge.edge_type == EdgeType.CONTRADICTS
        assert edge.confidence == 0.75

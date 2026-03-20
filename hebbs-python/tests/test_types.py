"""Tests for SDK public types."""

from __future__ import annotations

from hebbs.types import (
    Memory,
    MemoryKind,
    RecallResult,
    RecallOutput,
    StrategyDetail,
    ForgetResult,
    ReflectResult,
    HealthStatus,
    Edge,
    EdgeType,
)


def test_memory_creation():
    m = Memory(
        id=b"\x01\x02",
        content="test content",
        importance=0.8,
        context={"topic": "test"},
        entity_id="test_entity",
        kind=MemoryKind.EPISODE,
    )
    assert m.content == "test content"
    assert m.importance == 0.8
    assert m.entity_id == "test_entity"
    assert m.kind == MemoryKind.EPISODE
    assert m.context["topic"] == "test"


def test_recall_output():
    m = Memory(id=b"\x01", content="test", importance=0.5, context={})
    detail = StrategyDetail(strategy="similarity", relevance=0.9, distance=0.1)
    result = RecallResult(memory=m, score=0.9, strategy_details=[detail])
    output = RecallOutput(results=[result])
    assert len(output.results) == 1
    assert output.results[0].score == 0.9
    assert output.results[0].strategy_details[0].strategy == "similarity"


def test_forget_result():
    r = ForgetResult(forgotten_count=10, cascade_count=2, tombstone_count=10)
    assert r.forgotten_count == 10
    assert not r.truncated


def test_reflect_result():
    r = ReflectResult(insights_created=3, clusters_found=5, clusters_processed=5, memories_processed=50)
    assert r.insights_created == 3


def test_health_status():
    h = HealthStatus(serving=True, version="0.1.0", memory_count=100, uptime_seconds=3600)
    assert h.serving
    assert h.memory_count == 100


def test_edge_frozen():
    e = Edge(target_id=b"\x01", edge_type=EdgeType.CAUSED_BY, confidence=0.9)
    assert e.edge_type == EdgeType.CAUSED_BY


def test_memory_kind_values():
    assert MemoryKind.EPISODE.value == "episode"
    assert MemoryKind.INSIGHT.value == "insight"
    assert MemoryKind.REVISION.value == "revision"
    assert MemoryKind.DOCUMENT.value == "document"
    assert MemoryKind.PROPOSITION.value == "proposition"

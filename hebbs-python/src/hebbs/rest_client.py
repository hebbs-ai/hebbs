"""HebbsRestClient: async HTTP/REST client for HEBBS Enterprise.

Usage::

    async with HebbsRestClient("http://hebbs.acme.com:8080", api_key="hb_live_sk_...") as hb:
        mem = await hb.remember("Acme uses Salesforce", importance=0.8)
        results = await hb.recall("What CRM does Acme use?")
        print(results.text)
"""

from __future__ import annotations

import json
import os
from dataclasses import dataclass, field
from typing import Any

try:
    import aiohttp
except ImportError:
    aiohttp = None  # type: ignore[assignment]

try:
    from hebbs.exceptions import (
        HebbsAuthenticationError,
        HebbsConnectionError,
        HebbsError,
        HebbsInternalError,
        HebbsNotFoundError,
    )
except ImportError:
    # Standalone usage without full SDK
    class HebbsError(Exception): pass  # type: ignore[no-redef]
    class HebbsAuthenticationError(HebbsError): pass  # type: ignore[no-redef]
    class HebbsConnectionError(HebbsError): pass  # type: ignore[no-redef]
    class HebbsNotFoundError(HebbsError): pass  # type: ignore[no-redef]
    class HebbsInternalError(HebbsError): pass  # type: ignore[no-redef]


@dataclass
class RestMemory:
    """A memory returned from the REST API."""

    memory_id: str
    content: str
    importance: float = 0.0
    decay_score: float = 0.0
    entity_id: str | None = None
    file_path: str | None = None
    kind: str | None = None
    score: float = 0.0
    access_count: int = 0
    created_at_us: int = 0
    last_accessed_at_us: int = 0
    context: dict[str, Any] | None = None

    @classmethod
    def from_dict(cls, d: dict[str, Any]) -> RestMemory:
        return cls(
            memory_id=d.get("memory_id", ""),
            content=d.get("content", ""),
            importance=d.get("importance", 0.0),
            decay_score=d.get("decay_score", 0.0),
            entity_id=d.get("entity_id"),
            file_path=d.get("file_path"),
            kind=d.get("kind"),
            score=d.get("score", 0.0),
            access_count=d.get("access_count", 0),
            created_at_us=d.get("created_at_us", 0),
            last_accessed_at_us=d.get("last_accessed_at_us", 0),
            context=d.get("context"),
        )


@dataclass
class RestRecallOutput:
    """Results from a recall query."""

    results: list[RestMemory] = field(default_factory=list)
    count: int = 0
    indexing_pct: int | None = None

    @property
    def memories(self) -> list[RestMemory]:
        return self.results

    @property
    def text(self) -> str:
        """Concatenated content of all results, ready for LLM context."""
        return "\n\n".join(m.content for m in self.results)


@dataclass
class RestPrimeOutput:
    """Results from a prime query."""

    results: list[RestMemory] = field(default_factory=list)
    count: int = 0

    @property
    def memories(self) -> list[RestMemory]:
        return self.results

    @property
    def text(self) -> str:
        return "\n\n".join(m.content for m in self.results)


@dataclass
class RestInsightsOutput:
    """Insights for an entity."""

    insights: list[dict[str, Any]] = field(default_factory=list)
    count: int = 0

    @property
    def text(self) -> str:
        return "\n\n".join(
            i.get("content", str(i)) for i in self.insights
        )


@dataclass
class RestForgetResult:
    """Result of a forget operation."""

    forgotten_count: int = 0
    cascade_count: int = 0


@dataclass
class RestStatusOutput:
    """Server/workspace status."""

    status: str = ""
    version: str = ""
    engine: str = ""
    memories: int = 0
    files: int = 0


class HebbsRestClient:
    """Async REST client for HEBBS Enterprise.

    Args:
        endpoint: Server URL (e.g. ``http://hebbs.acme.com:8080``).
        api_key: API key for authentication. Falls back to ``HEBBS_API_KEY`` env var.
    """

    def __init__(
        self,
        endpoint: str = "http://localhost:8080",
        *,
        api_key: str | None = None,
    ) -> None:
        if aiohttp is None:
            raise ImportError("aiohttp is required for REST transport: pip install aiohttp")
        self._endpoint = endpoint.rstrip("/")
        self._api_key = api_key or os.environ.get("HEBBS_API_KEY", "")
        self._session: aiohttp.ClientSession | None = None

    async def connect(self) -> HebbsRestClient:
        headers: dict[str, str] = {}
        if self._api_key:
            headers["Authorization"] = f"Bearer {self._api_key}"
        self._session = aiohttp.ClientSession(
            base_url=self._endpoint,
            headers=headers,
        )
        return self

    async def close(self) -> None:
        if self._session:
            await self._session.close()
            self._session = None

    async def __aenter__(self) -> HebbsRestClient:
        return await self.connect()

    async def __aexit__(self, *exc: Any) -> None:
        await self.close()

    async def _request(self, method: str, path: str, body: dict | None = None) -> dict[str, Any]:
        if not self._session:
            raise HebbsConnectionError("Not connected. Use 'async with HebbsRestClient(...) as hb:'")

        kwargs: dict[str, Any] = {}
        if body is not None:
            kwargs["json"] = body

        async with self._session.request(method, path, **kwargs) as resp:
            data = await resp.json()
            if resp.status == 401:
                raise HebbsAuthenticationError(data.get("error", "Unauthorized"))
            if resp.status == 404:
                raise HebbsNotFoundError(data.get("error", "Not found"))
            if resp.status >= 400:
                raise HebbsInternalError(data.get("error", f"HTTP {resp.status}"))
            return data

    # ── Memory operations ──

    async def remember(
        self,
        content: str,
        importance: float | None = None,
        entity_id: str | None = None,
        context: dict[str, Any] | None = None,
    ) -> RestMemory:
        """Store a memory."""
        body: dict[str, Any] = {"content": content}
        if importance is not None:
            body["importance"] = importance
        if entity_id:
            body["entity_id"] = entity_id
        if context:
            body["context"] = context
        data = await self._request("POST", "/v1/memories", body)
        return RestMemory.from_dict(data)

    async def recall(
        self,
        cue: str,
        *,
        top_k: int = 10,
        entity_id: str | None = None,
        strategy: str | None = None,
    ) -> RestRecallOutput:
        """Search memories by semantic similarity."""
        body: dict[str, Any] = {"cue": cue, "top_k": top_k}
        if entity_id:
            body["entity_id"] = entity_id
        if strategy:
            body["strategy"] = strategy
        data = await self._request("POST", "/v1/recall", body)
        results = [RestMemory.from_dict(r) for r in data.get("results", [])]
        return RestRecallOutput(
            results=results,
            count=data.get("count", len(results)),
            indexing_pct=data.get("indexing_pct"),
        )

    async def prime(
        self,
        entity_id: str,
        *,
        max_memories: int = 30,
        similarity_cue: str | None = None,
    ) -> RestPrimeOutput:
        """Load all memories for an entity."""
        body: dict[str, Any] = {"entity_id": entity_id, "max_memories": max_memories}
        if similarity_cue:
            body["similarity_cue"] = similarity_cue
        data = await self._request("POST", "/v1/prime", body)
        results = [RestMemory.from_dict(r) for r in data.get("results", [])]
        return RestPrimeOutput(results=results, count=data.get("count", len(results)))

    async def forget(
        self,
        *,
        entity_id: str | None = None,
        ids: list[str] | None = None,
    ) -> RestForgetResult:
        """Delete memories."""
        body: dict[str, Any] = {}
        if entity_id:
            body["entity_id"] = entity_id
        if ids:
            body["ids"] = ids
        data = await self._request("POST", "/v1/forget", body)
        return RestForgetResult(
            forgotten_count=data.get("forgotten_count", 0),
            cascade_count=data.get("cascade_count", 0),
        )

    async def insights(self, entity_id: str | None = None) -> RestInsightsOutput:
        """Query entity insights."""
        path = "/v1/insights"
        if entity_id:
            path += f"?entity_id={entity_id}"
        data = await self._request("GET", path)
        return RestInsightsOutput(
            insights=data.get("insights", []),
            count=data.get("count", 0),
        )

    async def status(self) -> RestStatusOutput:
        """Get server health and workspace status."""
        data = await self._request("GET", "/v1/system/health")
        return RestStatusOutput(
            status=data.get("status", ""),
            version=data.get("version", ""),
            engine=data.get("engine", ""),
        )

    async def index(self, path: str) -> dict[str, Any]:
        """Upload files for indexing.

        Args:
            path: Local directory path containing files to upload.
        """
        import pathlib

        files_to_upload = []
        p = pathlib.Path(path)
        for f in p.rglob("*"):
            if f.is_file() and f.suffix in (".md", ".txt", ".pdf"):
                files_to_upload.append(f)

        if not files_to_upload:
            return {"uploaded": 0, "files": []}

        if not self._session:
            raise HebbsConnectionError("Not connected")

        data = aiohttp.FormData()
        for f in files_to_upload:
            data.add_field(
                "files",
                open(f, "rb"),
                filename=str(f.relative_to(p)),
            )

        async with self._session.post("/v1/upload", data=data) as resp:
            return await resp.json()

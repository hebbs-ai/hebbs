# Repository Structure: HEBBS

## Guiding Principle

A **Rust workspace monorepo** for the core engine. SDKs that link Rust code via native extensions (Python/PyO3) live inside the monorepo as workspace members. SDKs that are pure gRPC clients (TypeScript, Go) get **separate repos** for independent release cadences. A **docs repo** for the website.

---

## Repo 1: `hebbs` (Core Engine -- Rust Workspace Monorepo)

This is the heart of the project. A single Cargo workspace with multiple crates:

```
hebbs/
  Cargo.toml              (workspace root)
  crates/
    hebbs-core/           # Memory engine: remember, recall, revise, forget, reflect, decay
    hebbs-index/           # Three index implementations (temporal B-tree, HNSW vector, graph adjacency)
    hebbs-storage/         # RocksDB integration, column families, tiered storage (HOT/WARM/COLD)
    hebbs-embed/           # ONNX Runtime embedding engine, model management
    hebbs-reflect/         # Reflection pipeline: clustering, LLM proposal/validation, insight consolidation
    hebbs-server/          # Standalone binary: gRPC + HTTP/REST server, config, telemetry
    hebbs-proto/           # Protobuf definitions + tonic-generated Rust code
    hebbs-client/          # Rust client SDK (thin wrapper over gRPC)
    hebbs-ffi/             # C ABI for FFI (enables Python PyO3, embedded mode)
    hebbs-cli/             # Interactive CLI client (REPL + one-shot commands, like redis-cli)
    hebbs-python/          # Python SDK (PyO3 native extension + pure-Python package)
      Cargo.toml           #   Rust crate: cdylib "_hebbs_native"
      pyproject.toml       #   Maturin build config, PyPI metadata
      src/                 #   PyO3 Rust source (engine.rs, convert.rs, error.rs, subscribe.rs)
      hebbs/               #   Pure-Python package (shipped in wheel alongside .so/.dylib)
        __init__.py        #     HEBBS class, types, exceptions (public API)
        _types.py          #     Memory, RecallOutput, etc. dataclasses
        _exceptions.py     #     HebbsError hierarchy (10 exception classes)
        _native.py         #     Embedded mode backend wrapping NativeEngine
        aio/               #     Async API (server mode, future)
        integrations/      #     LangChain + CrewAI adapters (optional extras)
      tests/               #   pytest test suite (66 tests)
    hebbs-bench/           # Benchmark suite CLI (systems + cognitive benchmarks)
  hebbs-demo/              # Reference app: AI Sales Intelligence Agent (Python, Phase 14)
    pyproject.toml         #   Dependencies: hebbs, openai, anthropic, click, rich
    hebbs_demo/            #   CLI app, agent, memory manager, LLM client, scenarios
    tests/                 #   pytest scenario tests
    configs/               #   LLM provider config templates (Ollama, Anthropic, OpenAI)
  tests/                  # Integration tests
  benches/                # Criterion benchmarks
  docker/                 # Dockerfile, docker-compose
  proto/                  # .proto source files (shared with SDK repos)
```

**Why monorepo for core?** All Rust crates share the same compiler version, CI pipeline, and release cadence. Refactoring across crate boundaries is trivial. This is exactly what Qdrant, Meilisearch, and SurrealDB do.

**Why separate crates instead of one big crate?** Compile times, clear module boundaries, and the embedded library mode (`hebbs-ffi` + `hebbs-core`) should not pull in server dependencies.

**Why is `hebbs-python` inside the monorepo instead of a separate repo?** The PyO3 native extension links `hebbs-core` directly (no FFI indirection) — it needs workspace-level `Cargo.toml` access for path dependencies and shared compiler settings. Maturin's mixed Rust+Python layout works natively within the Cargo workspace. The Python source (`hebbs/`) lives alongside the Rust source (`src/`) in the same crate directory, producing a single wheel via `maturin build`.

---

## Repo 2: `hebbs-node`

- TypeScript/JavaScript SDK published to npm
- gRPC client (via `@grpc/grpc-js`) + REST fallback
- TypeScript-first with full type definitions
- Works in Node.js and edge runtimes (REST mode)

---

## Repo 3: `hebbs-go`

- Go SDK as a Go module
- gRPC client generated from shared `.proto` files
- Idiomatic Go patterns (context, options)

---

## Repo 4: `hebbs-docs`

- Documentation website (Docusaurus, Astro, or similar)
- API reference (auto-generated from proto files)
- Guides, tutorials, architecture deep-dives
- The `FINAL-README-WHEN-DONE.md` content lives here once polished
- Hosted on docs.hebbs.dev or similar

---

## Repo 5: `hebbs-deploy`

- Helm chart for Kubernetes
- Terraform modules (AWS, GCP, Azure)
- Docker Compose for local development
- Cloud-scale deployment configs (sharding, tiered storage)
- Monitoring dashboards (Grafana JSON, Prometheus rules)

---

## What NOT to Make a Separate Repo

| Concern | Where it lives | Why |
|---------|---------------|-----|
| Proto files | `hebbs/proto/` (core repo) | Source of truth; SDK repos pull from here via git submodule or CI copy |
| Python SDK | `hebbs/crates/hebbs-python/` | PyO3 links `hebbs-core` directly — needs workspace path deps, shared compiler settings, single `maturin build` |
| Benchmarks | `hebbs/crates/hebbs-bench/` | Tightly coupled to engine internals |
| CLI | `hebbs/crates/hebbs-cli/` | Separate binary from server (like `redis-cli` vs `redis-server`), but same repo — tightly coupled to proto definitions |
| Cloud platform | Future; starts as deployment configs | Don't build what you don't need yet |

---

## Summary

| Repo | Language | Purpose |
|------|----------|---------|
| `hebbs` | Rust + Python | Core engine, server, CLI, proto, benchmarks, FFI, **Python SDK** |
| `hebbs-node` | TypeScript | Node.js/TS SDK |
| `hebbs-go` | Go | Go SDK |
| `hebbs-docs` | MDX/JS | Documentation website |
| `hebbs-deploy` | YAML/HCL | Deployment configs (Helm, Terraform) |

Five repos total -- lean enough to manage as a small team, split enough to serve different communities and release cadences independently. The Python SDK lives inside the core monorepo because its PyO3 native extension links Rust crates directly.

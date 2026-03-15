# Phase 11: Python SDK -- Architecture Blueprint

## Status: COMPLETE

---

## Intent

Phase 11 is the inflection point where HEBBS transitions from a Rust systems project to a product that developers actually use. The Python AI/ML ecosystem is where agents live -- LangChain, CrewAI, AutoGen, DSPy, custom agent loops, Jupyter notebooks. If HEBBS cannot be used from Python with zero friction, the engineering quality of the preceding 10 phases is irrelevant. Nobody will know.

This phase produces a single PyPI package -- `hebbs` -- that gives Python developers the full HEBBS API in two modes:

**Server mode** -- `HEBBS("localhost:6380")` connects to a running `hebbs-server` via gRPC. Pure Python. No compilation. Works on any platform with a Python interpreter. The developer never sees protobuf, never constructs a gRPC channel, never handles byte-encoded memory IDs. They write `memory = hebbs.remember("the customer prefers email", importance=0.8)` and get back a Python dataclass.

**Embedded mode** -- `HEBBS.open("./agent-memory")` runs the full HEBBS engine in-process via PyO3-linked Rust code. No server process, no network, no deployment complexity. The agent script is self-contained. This is how most Python developers will first encounter HEBBS: a `pip install` and three lines of code in a notebook.

The decisions made here are load-bearing for four downstream concerns:

- **Adoption curve.** Python is the first language most AI developers will try HEBBS in. The API surface, error messages, documentation, and type hints established in this phase define the public perception of HEBBS. A clunky Python SDK will not be forgiven because the Rust internals are elegant. The SDK must feel like it was designed by a Python developer, not translated from Rust.

- **Framework integrations.** LangChain's `BaseMemory`, CrewAI's memory interface, and future framework adapters wrap the Python SDK. If the SDK's `recall()` returns a dict instead of a typed object, or if `subscribe()` requires manual threading, the framework adapters will be ugly and fragile. The core SDK API must be designed so that adapters are thin wrappers, not translation layers.

- **Phase 12 (Testing and Benchmark Suite).** The Python SDK is the third test vector (after Rust client SDK and FFI) that validates end-to-end correctness. Python integration tests that exercise server mode and embedded mode through the Python API are a high-signal correctness signal because they cross every abstraction boundary: Python → PyO3 → Rust → RocksDB → indexes, and Python → gRPC → server → engine.

- **Phase 16 (Documentation Site).** The "Getting Started" page will show Python code. The majority of HEBBS users will be Python developers. The SDK's import paths, class names, method signatures, and error messages must be polished to documentation-grade quality from day one. Renaming `HEBBSClient` to `HEBBS` after documentation is published is a breaking change with social cost.

---

## Scope Boundaries

### What Phase 11 delivers

- `hebbs` PyPI package installable via `pip install hebbs` on Linux (x86_64, aarch64) and macOS (arm64, x86_64)
- Server mode: pure-Python gRPC client wrapping `hebbs-proto` stubs, all 9 operations
- Embedded mode: PyO3-linked Rust engine (links `hebbs-core`, `hebbs-storage`, `hebbs-embed`, `hebbs-index`, `hebbs-reflect`), all 9 operations
- Unified API: identical method signatures for both modes, differing only in constructor
- Python-native types: dataclasses for `Memory`, `RecallResult`, `ForgetOutput`, `ReflectOutput`, `PrimeOutput`; `enum.Enum` for `MemoryKind`, `RecallStrategy`, `EdgeType`, `ContextMode`
- Type hints: full `py.typed` PEP 561 compliance, all public methods typed
- Sync API as primary surface, async API via `asyncio` for server mode
- Context manager protocol (`with HEBBS.open(...) as h:`) for embedded mode lifecycle
- Subscribe as iterator (sync) and async iterator (async)
- Custom exception hierarchy mapping Rust/gRPC errors to Python exceptions
- GIL release during all Rust-side computation in embedded mode
- Framework integrations as optional extras: `pip install hebbs[langchain]`, `pip install hebbs[crewai]`
- Maturin-based build system producing platform-specific wheels with pre-compiled Rust extensions

### What Phase 11 explicitly does NOT deliver

- Windows wheels (deferred to Phase 15 -- Windows RocksDB cross-compilation is a separate engineering effort)
- GPU-accelerated embedding in embedded mode (ONNX CPU execution provider only; GPU providers are Phase 17 edge/cloud configuration)
- Async embedded mode (PyO3 does not support native async Rust → Python async bridge without `pyo3-asyncio`, which adds complexity; embedded operations are synchronous with GIL released)
- Django/Flask/FastAPI middleware or integrations (these are downstream of the SDK, not part of it; a blog post suffices)
- Standalone model download CLI (the embedded mode downloads the ONNX model on first use, same as `hebbs-core`)
- Python-side caching or local index state (the SDK is stateless for server mode; embedded mode state lives in the Rust engine)
- Source distribution (`sdist`) that compiles Rust from source on the user's machine (only pre-built wheels; users without a matching wheel cannot install embedded mode but can use server mode via the pure-Python fallback)

---

## Architectural Decisions

### 1. One Package, Two Modes, Graceful Degradation

The temptation is to ship two packages: `hebbs` (pure Python, server mode only) and `hebbs-embedded` (PyO3, both modes). This mirrors Phase 10's two-crate split. It is wrong for Python.

**Decision: single `hebbs` package with an optional native extension. Server mode always works. Embedded mode works when the native extension is available.**

Rationale:

- **Discovery.** Python developers search PyPI for "hebbs". One result is better than two. Two packages create confusion about which to install, version synchronization headaches, and import path divergence.
- **Graceful fallback.** If a user's platform does not have a pre-built wheel (e.g., Alpine musl, 32-bit ARM), `pip install hebbs` still succeeds -- they get server mode. The native extension is optional. Calling `HEBBS.open()` on a platform without the extension raises `HebbsError("Embedded mode requires the native extension. Install a platform-specific wheel or use server mode: HEBBS('localhost:6380')")`. This is actionable, not cryptic.
- **Framework integrations import from one place.** `from hebbs.integrations.langchain import HebbsMemory` works regardless of mode. The adapter calls `hebbs.remember()` which dispatches to whichever backend is active. No conditional imports in user code.

The package structure:

```
hebbs/
  __init__.py          # Exports HEBBS, Memory, RecallStrategy, etc.
  _core.py             # Server mode implementation (pure Python)
  _native.py           # Embedded mode: imports PyO3 extension, wraps in Python API
  _types.py            # Dataclasses, enums, type definitions
  _exceptions.py       # Exception hierarchy
  _grpc/               # Generated gRPC stubs (grpcio + protobuf)
    __init__.py
    ...generated...
  integrations/
    __init__.py
    langchain.py        # LangChain BaseMemory adapter
    crewai.py           # CrewAI memory adapter
  py.typed              # PEP 561 marker
```

The PyO3 native extension compiles to `hebbs._hebbs_native` (underscore-prefixed, private). The public API never imports directly from the native extension. `_native.py` wraps it with Python types.

### 2. PyO3 Wrapping hebbs-core Directly, Not via hebbs-ffi

Phase 10 produced `hebbs-ffi` with a C ABI. The obvious path for Phase 11 is to wrap the C functions via `ctypes` or `cffi`. This is wrong.

**Decision: PyO3 links `hebbs-core` directly. The FFI layer is bypassed entirely.**

| Approach | JSON overhead per call | Type safety at boundary | Error fidelity | GIL release | Build complexity |
|----------|----------------------|------------------------|----------------|------------|-----------------|
| ctypes over hebbs-ffi | ~5-50µs (JSON ser/de) | None (raw pointers, caller manages lifetimes) | Error codes + string (lossy) | Manual (must wrap every call) | Low (link .so/.dylib) |
| cffi over hebbs-ffi | ~5-50µs (JSON ser/de) | Minimal (C header parsed) | Error codes + string (lossy) | Manual | Low |
| PyO3 over hebbs-ffi | ~5-50µs (JSON ser/de) | Partial (still JSON at boundary) | Error codes + string (lossy) | Automatic via `allow_threads` | Medium |
| **PyO3 over hebbs-core** | **~0.5-2µs (direct struct conversion)** | **Full (Rust types → Python types at compile time)** | **Full (Rust Result → Python exception with structured context)** | **Automatic via `allow_threads`** | **Medium** |

Why PyO3 over `hebbs-core` wins on every dimension:

- **No JSON serialization tax.** `hebbs-ffi` exchanges all structured data as JSON strings. A `Memory` struct is serialized to JSON in Rust, passed as `*const c_char` across the FFI boundary, then parsed from JSON in Python. With PyO3, a `Memory` struct is converted directly to a Python dict or dataclass via `IntoPyObject` / `FromPyObject` derives. The conversion is a field-by-field copy, not a parse. At agent-scale call rates (hundreds of recalls per conversation turn), the JSON overhead is measurable.

- **Full error fidelity.** `hebbs-ffi` collapses all errors to integer return codes (-1 through -6) with a thread-local error string. PyO3 can convert `hebbs_core::HebbsError` directly to a Python exception with the full variant structure intact. `HebbsError::NotFound { memory_id }` becomes `MemoryNotFoundError(memory_id="01ARZ3NDEKTSV4RRFFQ69G5FAV")` in Python with the memory ID accessible as an attribute. The FFI path can only produce `HebbsError("Not found: ...")` with the ID buried in a string.

- **GIL release is ergonomic.** PyO3's `py.allow_threads(|| { ... })` releases the GIL for the duration of the Rust closure. Every engine operation (remember, recall, reflect) runs with the GIL released, allowing other Python threads to proceed. With ctypes/cffi, every function call requires manual GIL release wrappers, which is error-prone and easy to forget.

- **Compile-time type checking.** PyO3's `#[pyfunction]` and `#[pyclass]` attributes are checked at compile time. A type mismatch between the Rust function signature and the Python-facing API is a compile error, not a runtime crash. With ctypes, a wrong argument type is a segfault.

**Why not reuse hebbs-ffi at all:** The FFI layer exists for languages that cannot link Rust directly -- C, C++, Go (via cgo), Ruby (via FFI gem). Python has PyO3, which is a strictly superior integration path. Using the FFI from Python would be accepting unnecessary overhead and losing type information for no benefit. The FFI layer is not wasted -- it serves its intended consumers (C/C++ embeddings, Phase 19 Go SDK). The Python SDK simply does not need it.

**The PyO3 crate lives inside the hebbs workspace** as `crates/hebbs-python/` using maturin's mixed Rust+Python layout (Option C). The directory structure is:

```
crates/hebbs-python/
  Cargo.toml              # Rust crate: [lib] name = "_hebbs_native", crate-type = ["cdylib"]
  pyproject.toml           # Maturin build config, PyPI metadata, dependencies
  src/
    lib.rs                 # PyO3 module definition
    engine.rs              # NativeEngine #[pyclass] wrapping hebbs-core::Engine
    convert.rs             # Memory/Result → Python dict conversion
    error.rs               # HebbsError → PyErr mapping
    subscribe.rs           # Subscribe handle wrapping
  hebbs/                   # Pure-Python package (shipped in wheel alongside .so/.dylib)
    __init__.py            # Public API: HEBBS class, types, exceptions
    _types.py              # Memory, RecallResult, etc. dataclasses
    _exceptions.py         # HebbsError hierarchy
    _native.py             # Embedded mode: wraps _hebbs_native.NativeEngine
    _grpc.py               # Server mode: pure-Python gRPC client
    aio/
      __init__.py          # Async API: HEBBS class for asyncio
    integrations/
      __init__.py
      langchain.py         # LangChain BaseMemory adapter
      crewai.py            # CrewAI memory adapter
    py.typed               # PEP 561 marker
```

The workspace `Cargo.toml` includes `"crates/hebbs-python"` as a member. Maturin builds the Rust crate and packages it alongside the `hebbs/` Python source into a single wheel.

### 3. The Unified Entry Point -- `HEBBS` Class

Python developers should not need to decide between `HebbsClient`, `HebbsEngine`, `HebbsEmbedded`, or `HebbsServer`. There is one class: `HEBBS`.

**Decision: `HEBBS` is the single entry point. The constructor determines the mode. All methods are identical.**

```python
# Server mode
h = HEBBS("localhost:6380")

# Embedded mode
h = HEBBS.open("./agent-memory")

# Embedded mode with configuration
h = HEBBS.open("./agent-memory", embedding_model="/path/to/model.onnx", decay_half_life_days=14)
```

Internally, `HEBBS.__init__` stores a backend reference that implements a private `_Backend` protocol. Server mode instantiates `_GrpcBackend`. Embedded mode instantiates `_NativeBackend` (which holds a PyO3 `Engine` wrapper). All public methods delegate to `self._backend.operation(...)`. The backend is an implementation detail invisible to the user.

Why a class method (`HEBBS.open()`) for embedded mode instead of a constructor parameter:

- **Semantic clarity.** `HEBBS("localhost:6380")` connects to something. `HEBBS.open("./data")` opens something. The verb communicates the lifecycle implication: `open` implies `close`, which implies resource management. A string endpoint implies a lightweight connection.
- **Resource management.** Embedded mode owns a RocksDB database, an ONNX runtime session, background threads (decay, reflect). It requires explicit cleanup. The class method pattern naturally pairs with `HEBBS.close()` and the context manager protocol (`with HEBBS.open(...) as h:`). Server mode does not require explicit cleanup -- the gRPC channel is lightweight.
- **Configuration divergence.** Server mode needs `endpoint`, `timeout`, `retry`. Embedded mode needs `data_dir`, `embedding_model`, `hnsw_m`, `decay_half_life_days`. These are different parameter sets. Cramming them into one constructor with mode-dependent parameters is confusing. Separate constructors cleanly separate the concerns.

### 4. Python Type Mapping -- The Conversion Contract

Every Rust type that crosses the PyO3 boundary must have an exact Python representation. This mapping is the API contract that framework integrations, user code, and documentation depend on. Getting it wrong means a breaking change later.

**Decision: Python-native types for all public API surfaces. No protobuf types, no Rust wrapper objects.**

| Rust type (hebbs-core) | Python type | Rationale |
|------------------------|-------------|-----------|
| `Memory` struct | `@dataclass Memory` | Dataclasses are the Python standard for structured data. IDE autocomplete, type checking, JSON serialization via `dataclasses.asdict()` all work automatically. |
| `Ulid` (memory_id) | `str` (26-character ULID) | Python developers expect string IDs. ULIDs are string-representable by design. `memory.id` returns `"01ARZ3NDEKTSV4RRFFQ69G5FAV"`. No custom ID type to learn. |
| `HashMap<String, Value>` (context) | `dict[str, Any]` | Python dicts are the natural representation. JSON-compatible values (str, int, float, bool, list, dict, None) map directly. |
| `MemoryKind` enum | `enum.Enum` subclass | `MemoryKind.EPISODE`, `MemoryKind.INSIGHT`, `MemoryKind.REVISION`. Python enum conventions use UPPER_CASE. |
| `RecallStrategy` enum | `enum.Enum` subclass | `RecallStrategy.SIMILARITY`, `RecallStrategy.TEMPORAL`, `RecallStrategy.CAUSAL`, `RecallStrategy.ANALOGICAL`. |
| `EdgeType` enum | `enum.Enum` subclass | `EdgeType.CAUSED_BY`, `EdgeType.RELATED_TO`, etc. |
| `Option<T>` | `T | None` | Standard Python optional. |
| `Vec<f32>` (embedding) | `list[float]` | Python lists. NumPy array conversion is possible but adds a mandatory dependency. Users who want NumPy can wrap: `np.array(memory.embedding)`. |
| `u64` (timestamps) | `int` | Python ints are unbounded. Microsecond epoch timestamps fit naturally. |
| `f32` (importance, decay_score) | `float` | Python floats are f64. Precision loss from f32→f64 is acceptable (rounding, not truncation). |
| `Result<T, HebbsError>` | `T` or raises exception | Pythonic error handling. No `Result` objects in Python. |
| Duration (timeout) | `float` (seconds) | Python convention for timeouts. `timeout=5.0` means 5 seconds. |

**Memory ID as string, not bytes:** The Rust client SDK uses `Ulid` (a typed wrapper). The FFI uses 26-character ULID strings. The Python SDK uses `str`. This is deliberate: Python developers do not want to import a ULID library to use HEBBS. Memory IDs are opaque identifiers that happen to be sortable strings. The SDK validates ULID format on input (26 alphanumeric characters, Crockford base32) and rejects invalid IDs with `InvalidInputError` before they reach the engine.

**Embedding as list, not numpy array:** Adding numpy as a required dependency would force every HEBBS user to install numpy, even if they never inspect embeddings. Embeddings are rarely accessed directly by users -- they are internal to the engine's similarity search. For the rare case where a user wants the raw embedding, `memory.embedding` returns `list[float]` and they convert to numpy themselves. This follows the same principle as the Rust SDK: the optional field exists but does not impose dependencies.

### 5. Exception Hierarchy

Python exceptions are the error reporting mechanism. A flat `HebbsError` for all failures is as useless as a flat `Exception`. Framework integrations need to catch specific error types to implement retry logic, fallback behavior, and error reporting.

**Decision: hierarchical exception classes mirroring the Rust error taxonomy and the client SDK's `ClientError` variants.**

```
HebbsError (base)
├── ConnectionError          # Server unreachable, DNS failure, TLS failure
├── TimeoutError             # Operation exceeded deadline
├── MemoryNotFoundError      # Memory ID does not exist (carries memory_id attribute)
├── InvalidInputError        # Bad arguments: content too long, importance out of range, invalid ULID
├── StorageError             # RocksDB I/O failure (embedded mode)
├── EmbeddingError           # ONNX model failure (embedded mode)
├── ServerError              # Server-side bug
├── RateLimitedError         # Rate limit exceeded (carries retry_after_seconds if available)
├── SubscriptionClosedError  # Subscribe stream ended unexpectedly
└── ConfigurationError       # Invalid open/connect configuration
```

Each exception carries structured attributes beyond the message string:

- `MemoryNotFoundError.memory_id` -- the ID that was not found
- `TimeoutError.operation` -- which operation timed out
- `TimeoutError.elapsed_seconds` -- how long it waited
- `ConnectionError.endpoint` -- what it tried to connect to
- `RateLimitedError.retry_after_seconds` -- when to retry (if server provides)
- `InvalidInputError.field` -- which parameter was invalid
- `InvalidInputError.constraint` -- what the constraint was ("max 65536 bytes", "range [0.0, 1.0]")

These attributes enable structured error handling in framework adapters without parsing error message strings.

**Exception mapping from Rust:** In embedded mode, PyO3 catches `hebbs_core::HebbsError` variants and converts them to the corresponding Python exception using a `From<HebbsError> for PyErr` implementation. In server mode, gRPC status codes are mapped to exceptions using the same mapping as `hebbs-client`'s `ClientError` (Phase 10, Decision 5).

### 6. GIL Management Strategy

The Global Interpreter Lock (GIL) is the single most important performance concern in the Python SDK. If the GIL is held during Rust engine operations, no other Python thread can make progress. For an agent runtime that processes multiple conversations concurrently, GIL contention would make HEBBS a serialization bottleneck.

**Decision: release the GIL for every Rust-side operation in embedded mode. Never hold the GIL during I/O, embedding, or index operations.**

PyO3's `py.allow_threads(|| { ... })` releases the GIL for the duration of the closure. The pattern for every embedded-mode operation:

1. Extract all arguments from Python objects into Rust-owned values (GIL held, fast).
2. Release the GIL.
3. Execute the engine operation (GIL released, potentially slow).
4. Reacquire the GIL.
5. Convert the Rust result to Python objects (GIL held, fast).

This means the Rust engine code never interacts with Python objects. All data crossing the boundary is converted to Rust-owned types before the GIL is released, and converted back to Python types after the GIL is reacquired. No `PyObject` or `Py<T>` references exist inside the engine call.

**Server mode GIL behavior:** The gRPC client (grpcio) releases the GIL internally during network I/O. No additional GIL management is needed for server mode. The Python thread is blocked on the gRPC call but other threads can proceed.

**Subscribe callback GIL interaction:** In embedded mode, the subscribe callback is invoked from a Rust background thread. PyO3 requires GIL acquisition to call Python code. The subscribe worker acquires the GIL only for the duration of the callback invocation, then releases it. This means the subscribe background thread does not permanently hold the GIL. If the callback is slow (e.g., the user does heavy processing in the callback), GIL contention is the user's problem, not the SDK's.

### 7. Async API Design

Python has two worlds: synchronous (threading, `time.sleep`, `requests`) and asynchronous (`asyncio`, `await`, `aiohttp`). Most agent frameworks support both. The SDK must serve both worlds without forcing a choice.

**Decision: synchronous API as the primary surface. Async wrappers for server mode as `hebbs.aio`. No async for embedded mode.**

| Mode | Sync API | Async API |
|------|----------|-----------|
| Server (`HEBBS("endpoint")`) | `h.remember(...)` blocks until gRPC response | `await h.remember(...)` via `hebbs.aio.HEBBS` |
| Embedded (`HEBBS.open(path)`) | `h.remember(...)` blocks (GIL released) | Not provided (see rationale) |

**Why sync is primary:**

- Most Python developers write synchronous code. LangChain's `BaseMemory` is synchronous. CrewAI's memory interface is synchronous. The first HEBBS integration a developer writes will be synchronous.
- Jupyter notebooks are synchronous. The "try HEBBS in 5 minutes" experience must be synchronous.
- Synchronous code is easier to debug, test, and reason about.

**Why async for server mode only:**

- Server mode is I/O-bound (network calls). Async genuinely improves throughput for I/O-bound operations by allowing the event loop to multiplex while waiting for gRPC responses.
- Embedded mode is CPU-bound (embedding generation, HNSW search). Async provides no benefit for CPU-bound work -- the GIL still serializes Python code, and the Rust side already releases the GIL. Wrapping CPU-bound work in `async` just adds overhead (`asyncio.to_thread` or `loop.run_in_executor`).

**Async API surface:**

```python
from hebbs.aio import HEBBS

async def agent_loop():
    h = await HEBBS.connect("localhost:6380")
    memory = await h.remember("customer prefers email", importance=0.8)
    results = await h.recall("contact preferences")
```

The async `HEBBS` class lives in `hebbs.aio` (following `aiohttp`, `aioredis`, `aiofiles` convention). It wraps the same gRPC stubs but uses `grpcio.aio` (grpcio's native asyncio support) instead of synchronous `grpcio`. The async subscribe returns an `AsyncIterator`.

### 8. Subscribe Model in Python

Subscribe is the most complex operation to expose idiomatically in Python. The Rust SDK returns an async `Stream`. The FFI uses callbacks. Python needs something that feels native.

**Decision: synchronous subscribe returns an iterator. Async subscribe returns an async iterator. Both support context manager protocol.**

**Synchronous subscribe (both modes):**

```python
with h.subscribe(entity_id="customer_123", confidence_threshold=0.7) as stream:
    stream.feed("The customer mentioned they want faster delivery")
    for push in stream:
        print(f"Relevant memory: {push.memory.content} (confidence: {push.confidence})")
```

The `SubscribeStream` object:
- Implements `__iter__` and `__next__` (blocking iteration, returns `SubscribePush` objects).
- Implements `__enter__` and `__exit__` (context manager for cleanup).
- Has `feed(text)` method for pushing text to the subscription.
- Has `close()` method for explicit cleanup.
- `__next__` blocks until a push is available or the subscription ends (raises `StopIteration`).
- Timeout per `next()` call is configurable via constructor parameter `poll_timeout_seconds` (default: no timeout, blocks indefinitely).

**Async subscribe (server mode):**

```python
async with h.subscribe(entity_id="customer_123") as stream:
    await stream.feed("The customer mentioned delivery speed")
    async for push in stream:
        print(f"Relevant: {push.memory.content}")
```

The async `SubscribeStream` implements `__aiter__` and `__anext__`, yielding `SubscribePush` objects.

**Embedded mode subscribe internals:** The Rust `Engine::subscribe()` returns a `SubscriptionHandle` with `feed()`, `try_recv()`, and `close()`. The PyO3 wrapper holds this handle. The Python `__next__` method calls `try_recv()` in a polling loop with GIL release between polls, using `threading.Event` for wakeup signaling to avoid busy-waiting.

### 9. Framework Integration Architecture

Framework integrations are optional extras that wrap the core SDK. They must be thin -- the core SDK does the work, the integration adapts the interface.

**Decision: integrations are Python modules in `hebbs/integrations/`, each with its own optional dependency group. They import `hebbs` core types and wrap a `HEBBS` instance.**

**LangChain integration (`pip install hebbs[langchain]`):**

Implements LangChain's `BaseMemory` interface:
- `load_memory_variables(inputs)` → calls `h.recall(inputs["input"])` and returns memories as a dict
- `save_context(inputs, outputs)` → calls `h.remember(outputs["output"])` with context extracted from inputs
- `clear()` → calls `h.forget(ForgetCriteria(entity_id=self.entity_id))`

Also implements `VectorStore` protocol for use as a retriever:
- `add_texts(texts)` → batch `h.remember()` calls
- `similarity_search(query, k)` → `h.recall(query, strategy=RecallStrategy.SIMILARITY, top_k=k)`

**CrewAI integration (`pip install hebbs[crewai]`):**

Implements CrewAI's memory interfaces:
- `ShortTermMemory` → `h.recall()` with temporal strategy and recent time window
- `LongTermMemory` → `h.recall()` with similarity strategy
- `EntityMemory` → `h.recall()` with entity_id scope

**Integration design principles:**

- Integrations never import `hebbs._native` or `hebbs._grpc`. They use the public `hebbs.HEBBS` class exclusively.
- Integrations accept a `HEBBS` instance in their constructor. They do not create their own connections. This allows the user to share a single HEBBS connection across multiple framework integrations.
- Integrations are tested against both server mode and embedded mode via the same test suite (same assertions, different `HEBBS` constructor).

### 10. Build System and Wheel Strategy

The build system determines what users actually receive when they run `pip install hebbs`. Getting this wrong means broken installs, missing native extensions, or 200MB wheel sizes.

**Decision: maturin as the build backend. Mixed Python/Rust project layout. One wheel per platform with the native extension, plus a universal pure-Python wheel as fallback.**

**Maturin configuration:**

Maturin is the standard build tool for PyO3 projects. It produces wheels that contain both the compiled Rust extension (`.so` / `.dylib` / `.pyd`) and the pure-Python package directory. The `pyproject.toml` declares maturin as the build backend:

```toml
[build-system]
requires = ["maturin>=1.0,<2.0"]
build-backend = "maturin"

[project]
name = "hebbs"
requires-python = ">=3.9"
dependencies = ["grpcio>=1.60", "protobuf>=4.25"]

[project.optional-dependencies]
langchain = ["langchain-core>=0.2"]
crewai = ["crewai>=0.1"]
```

**Wheel matrix:**

| Platform | Architecture | Wheel tag | Size estimate | Contains native extension |
|----------|-------------|-----------|--------------|--------------------------|
| Linux glibc | x86_64 | `manylinux_2_28_x86_64` | ~25-35MB | Yes (includes RocksDB, ONNX RT) |
| Linux glibc | aarch64 | `manylinux_2_28_aarch64` | ~25-35MB | Yes |
| macOS | arm64 | `macosx_11_0_arm64` | ~20-30MB | Yes |
| macOS | x86_64 | `macosx_10_12_x86_64` | ~20-30MB | Yes |
| Any | Any | `py3-none-any` | ~200KB | No (server mode only) |

The pure-Python fallback wheel (`py3-none-any`) contains only the Python source -- no compiled Rust code. It supports server mode exclusively. `HEBBS.open()` raises `HebbsError("Native extension not available. Install a platform-specific wheel for embedded mode.")`. This wheel is last in pip's resolution order (platform-specific wheels are preferred).

**Why manylinux_2_28, not manylinux_2_17:** RocksDB requires glibc ≥ 2.28 for `statx()` and other modern syscalls. manylinux_2_17 (CentOS 7 era) would require patching RocksDB or statically linking an older glibc. The pragmatic choice is manylinux_2_28 (Ubuntu 20.04+, Debian 11+, RHEL 8+), which covers the vast majority of production Python environments.

**ONNX model is NOT bundled in the wheel.** The BGE-small-en-v1.5 ONNX model is ~33MB. Including it in every wheel would double the download size for a file that is downloaded once and cached. The embedded mode downloads the model on first use (same behavior as `hebbs-core`), with SHA-256 verification and local caching. The model path is configurable via `HEBBS.open(embedding_model="/path/to/model.onnx")`.

### 11. Server Mode gRPC Client Implementation

Server mode must work without the native extension. This means pure-Python gRPC client code.

**Decision: use `grpcio` and `grpcio-tools` to generate Python stubs from the same `.proto` files used by `hebbs-proto`. Wrap the stubs with Python-native types.**

The generated stubs (`*_pb2.py`, `*_pb2_grpc.py`) are committed to the repository (not generated at install time) to avoid requiring `grpcio-tools` as an install dependency. They are regenerated when `.proto` files change, tracked as a CI check.

The server mode implementation (`_core.py`) follows the same patterns as `hebbs-client` (Phase 10):

- Builder-style connection: `HEBBS("localhost:6380", timeout=5.0, retry_max=3)`
- Retry with exponential backoff for idempotent operations (get, recall, prime, insights)
- No retry for non-idempotent operations (remember, revise, forget, reflect)
- gRPC status code → Python exception mapping identical to the Rust client SDK mapping

The type conversion layer converts between protobuf types and Python dataclasses. This layer mirrors `hebbs-client`'s `convert.rs` but in Python.

### 12. Embedded Mode PyO3 Architecture

The PyO3 extension is a Rust crate (`crates/hebbs-python/`) that compiles to a shared library importable by Python.

**Decision: PyO3 exposes a single `NativeEngine` class with methods for all 9 operations plus lifecycle. The Python-side `_native.py` wraps it with Python-native types.**

The layering:

1. **`crates/hebbs-python/src/lib.rs`** -- Rust code. Defines `#[pyclass] NativeEngine` with `#[pymethods]` for `open`, `close`, `remember`, `recall`, `revise`, `forget`, `prime`, `subscribe`, `reflect`, `insights`, `count`. Each method extracts Python arguments, releases the GIL, calls `Engine`, reacquires the GIL, converts the result to a Python dict. All results are returned as Python dicts (not custom PyO3 classes), because dict-to-dataclass conversion is trivial in Python and avoids PyO3's limited dataclass support.
2. **`hebbs/_native.py`** -- Python code. Imports `hebbs._hebbs_native.NativeEngine`. Wraps each method to convert dict results to `Memory`, `RecallOutput`, etc. dataclass instances. Handles exception conversion. This layer is where the Python API polish happens.
3. **`hebbs/__init__.py`** -- Delegates to `_native.py` or `_core.py` based on the constructor used.

**Why dicts at the PyO3 boundary, not PyO3 classes:** Defining `Memory` as a `#[pyclass]` in Rust would give Python a Rust-backed object. This has limitations: no `dataclasses.asdict()` support, no `__dict__`, limited IDE introspection, serialization requires custom implementations. Converting to a Python dict in Rust and then to a dataclass in Python gives full Python-native objects with zero limitations. The dict-to-dataclass conversion cost is ~1µs -- negligible compared to the engine operation.

### 13. Dependency Graph

**Server mode (pure-Python) dependencies:**

| Package | Version | Purpose |
|---------|---------|---------|
| `grpcio` | ≥ 1.60 | gRPC client transport |
| `protobuf` | ≥ 4.25 | Protobuf message serialization |

Two dependencies. That is the floor for gRPC communication. No additional packages.

**Embedded mode (Rust, linked at build time):**

| Rust crate | Purpose |
|------------|---------|
| `pyo3` | Python ↔ Rust binding |
| `hebbs-core` | Engine (remember, recall, revise, forget, prime, subscribe, reflect, insights) |
| `hebbs-storage` | RocksDB backend |
| `hebbs-embed` | ONNX embedding |
| `hebbs-index` | HNSW, temporal, graph indexes |
| `hebbs-reflect` | Reflection pipeline |

All Rust dependencies are statically linked into the native extension `.so`/`.dylib`. The Python user does not install Rust, RocksDB, or ONNX Runtime. Everything is in the wheel.

**Optional dependencies:**

| Extra | Package | Version |
|-------|---------|---------|
| `langchain` | `langchain-core` | ≥ 0.2 |
| `crewai` | `crewai` | ≥ 0.1 |

**Dependencies NOT added (and why):**

| Considered | Rejected because |
|-----------|-----------------|
| `numpy` | Would force all users to install numpy. Embeddings are rarely inspected directly. Users who want numpy arrays can convert `list[float]` themselves. |
| `pydantic` | Would add a heavy dependency for data validation that dataclasses + manual validation handle adequately. Pydantic v1/v2 compatibility issues would be inherited. |
| `ulid-py` | Memory IDs are strings. A ULID library adds a dependency for a type the user never constructs (IDs are generated by the engine). |
| `tenacity` | Retry logic is 20 lines of code. Does not justify a dependency. |
| `grpcio-reflection` | Server-side concern, not client-side. |

### 14. Versioning and Compatibility Contract

**Decision: the Python SDK version tracks the HEBBS engine version. `hebbs==0.11.x` corresponds to Phase 11. The Python SDK is compatible with any `hebbs-server` at the same major version.**

Python version support: ≥ 3.9. Python 3.8 reached end of life in October 2024. Supporting 3.9+ covers all active Python versions and enables modern type hint syntax (`dict[str, Any]` instead of `Dict[str, Any]`, `X | None` instead of `Optional[X]`).

**Wire compatibility:** The Python SDK (server mode) speaks protobuf over gRPC against `hebbs-server`. The `.proto` files are the compatibility contract. Adding fields is backward-compatible. Removing fields requires a major version bump. The Python SDK must tolerate unknown fields in proto responses (protobuf's default behavior).

**Embedded mode compatibility:** The Python SDK (embedded mode) links `hebbs-core` at compile time. The database format is the compatibility contract. A database created by `hebbs-core` version X must be readable by version X+1. Backward-incompatible schema changes require a migration (see Phase 1, Decision 3: schema evolution strategy).

### 15. Performance Boundaries

The Python SDK adds overhead relative to the Rust engine. This overhead must be bounded and measured.

**Budget per operation (overhead ABOVE the engine/server latency):**

| Component | Budget | Phase 10 (Rust client) reference |
|-----------|--------|--------------------------------|
| PyO3 argument extraction (embedded) | < 5µs | N/A (direct Rust call) |
| PyO3 result conversion (embedded) | < 10µs | N/A |
| GIL release/reacquire (embedded) | < 1µs | N/A |
| Proto serialization (server) | < 20µs | < 5µs (Rust proto) |
| Dict → dataclass conversion (Python) | < 5µs | N/A |
| **Total embedded overhead** | **< 20µs** | ~0 (same process) |
| **Total server overhead** | **< 50µs** | < 10µs (Rust client) |

These budgets are generous. The engine operations (remember ~5ms, recall ~10ms) dominate. The SDK overhead is < 1% of end-to-end latency. If profiling in Phase 12 shows otherwise, the first optimization target is the dict-to-dataclass conversion (can be replaced with `__slots__` classes or direct attribute assignment).

---

## Risk Register

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| PyO3 version incompatibility with specific Python versions (3.9–3.13) | High -- wheel fails to import on some Python versions, broken installs | Medium (PyO3 tracks CPython closely but abi3 edge cases exist) | Build and test wheels against every supported Python version in CI. Use PyO3's `abi3` feature for stable ABI (single wheel per platform, compatible with all Python versions ≥ 3.9). If abi3 causes issues, fall back to per-version wheels. |
| Wheel size exceeds PyPI upload limit (100MB) or is too large for ergonomic `pip install` | Medium -- users wait or fail to install | Low (estimated 25-35MB, well under limit) | Strip debug symbols in release builds (`strip = true` in Cargo profile). Use `opt-level = "s"` if size exceeds 50MB. Exclude ONNX model from wheel (already decided). Monitor wheel size in CI with a hard gate at 50MB. |
| ONNX Runtime's dynamic library linking conflicts with other Python packages that bundle ONNX RT (e.g., `onnxruntime` pip package) | High -- symbol conflicts, segfaults, or wrong version loaded at runtime | Medium (ONNX RT is increasingly bundled by ML packages) | Statically link ONNX Runtime into the native extension. Verify no exported ONNX RT symbols leak from the `.so`/`.dylib` using `nm` in CI. If symbol conflicts are unavoidable, namespace ONNX RT symbols via linker version scripts. |
| GIL reacquisition in subscribe callback creates deadlock with user code that holds the GIL | Medium -- process hangs | Low (requires user to hold GIL and block on subscribe in the same thread, which is unusual) | Document that subscribe callbacks must not block on GIL-acquiring operations. Detect deadlock scenarios with a timeout on GIL acquisition in the subscribe callback path (5 second timeout, then log warning and skip the push). |
| `grpcio` version conflicts with user's existing grpcio installation (different version requirements) | Medium -- pip resolution failure or runtime errors | Medium (grpcio is widely used with varying version pins) | Specify a wide version range (`>=1.60`) and test against the latest grpcio in CI. If conflicts become common, provide an alternative transport backend using `grpclib` (pure-Python gRPC) as a future option. |
| maturin build fails in CI for cross-compilation targets (e.g., building aarch64 wheels on x86_64 CI runners) | Medium -- missing platform wheels | Medium (cross-compilation for RocksDB and ONNX RT is nontrivial) | Use platform-native CI runners (GitHub Actions `ubuntu-latest` for x86_64, `macos-14` for arm64). For Linux aarch64, use QEMU emulation or cross-rs with a manylinux_2_28 Docker image. Test wheel installation on target platforms in CI. |
| Python SDK types diverge from Rust client SDK types (same semantic field, different name or behavior) | High -- confusing cross-language inconsistency | Medium | Every Python type has a docstring citing the corresponding Rust client SDK type. A CI check compares Python dataclass field names against Rust struct field names (automated parity test). |
| Embedded mode database locking prevents concurrent Python processes from opening the same data directory | Medium -- confusing error for users who expect multi-process access | High (RocksDB uses file-level locking by design) | Document prominently that embedded mode is single-process. The `HEBBS.open()` error message when the lock fails is actionable: "Database at './agent-memory' is locked by another process. Use server mode for multi-process access: HEBBS('localhost:6380')". |

---

## Testing Strategy

### Layer 1: Unit tests (Python, no engine or server required)

- **Type construction and validation:** Verify `Memory`, `RecallResult`, `ForgetOutput`, `PrimeOutput`, `ReflectOutput` dataclasses construct correctly from dicts. Verify enum members exist and display correctly. Verify `Memory.__eq__` compares by `id` field.
- **Exception hierarchy:** Verify all exception classes are subclasses of `HebbsError`. Verify structured attributes (`.memory_id`, `.operation`, `.endpoint`) are accessible. Verify `str(exception)` produces actionable messages.
- **Input validation:** Verify `HEBBS.open("")` raises `ConfigurationError`. Verify `h.remember("", importance=0.5)` raises `InvalidInputError` for empty content. Verify `h.remember("text", importance=1.5)` raises `InvalidInputError` for out-of-range importance. Verify invalid ULID strings as memory IDs raise `InvalidInputError`.
- **Type conversion round-trips:** Verify dict → Memory → dict preserves all fields. Verify proto → Python → proto preserves all fields (server mode conversion layer).
- **Config parsing:** Verify `HEBBS("localhost:6380", timeout=5.0)` stores configuration correctly. Verify `HEBBS.open("./data", decay_half_life_days=14)` produces the correct JSON configuration for the Rust engine.

### Layer 2: Property-based tests (hypothesis)

- **Memory round-trip fidelity:** For any valid dict of Memory fields (generated by hypothesis strategies), `Memory(**d).__dict__` preserves all values within type-appropriate tolerances (f32→f64 rounding for floats).
- **ULID string validation:** For any 26-character Crockford Base32 string, the ULID validator accepts it. For any string that is not a valid ULID, the validator rejects it.
- **Context dict round-trip:** For any JSON-serializable dict (generated by hypothesis), the dict survives the Python → Rust → Python conversion path in embedded mode without data loss.
- **Exception message completeness:** For any exception subclass, `str(exception)` contains all structured attribute values (no silent data loss in error formatting).

### Layer 3: Integration tests -- Embedded mode

Start an embedded HEBBS engine via `HEBBS.open(tempdir)`. Run the full operation lifecycle:

- **Full lifecycle:** `remember` → `recall` (all 4 strategies) → `revise` → `recall` (verify update) → `forget` → `recall` (verify empty). Verify return types are Python dataclasses, not dicts or Rust wrapper objects.
- **Context round-trip:** Remember a memory with nested context dict, recall it, verify context dict is identical (including nested lists, booleans, null values).
- **Subscribe lifecycle:** Open subscription, feed text, receive pushes, close subscription. Verify push objects are `SubscribePush` dataclasses.
- **Concurrent operations:** 4 Python threads sharing one `HEBBS` handle, each performing 100 remember + recall cycles. Verify no data corruption and no GIL deadlocks.
- **Context manager:** Verify `with HEBBS.open(tempdir) as h:` cleans up properly (database directory is unlocked after exit).
- **Error handling:** Verify `h.recall("query")` on an empty database returns empty results (not an exception). Verify `h.get("invalid-ulid")` raises `InvalidInputError`. Verify `h.get("01ARZ3NDEKTSV4RRFFQ69G5FAV")` raises `MemoryNotFoundError` with `.memory_id` attribute.
- **Reflect + insights:** Remember 100+ memories, trigger `h.reflect(entity_id="test")`, query `h.insights(entity_id="test")`, verify insights are `Memory` objects with `kind == MemoryKind.INSIGHT`.

### Layer 4: Integration tests -- Server mode

Start `hebbs-server` as a subprocess with in-memory backend. Run the same logical test suite as embedded mode but via `HEBBS("localhost:{port}")`.

- Verify all 9 operations return the same result types as embedded mode.
- Verify retry behavior: kill and restart the server mid-test, verify idempotent operations (recall, get) recover after retry.
- Verify timeout behavior: configure 100ms timeout, recall against a large dataset, verify `TimeoutError` is raised.
- Verify connection recovery: stop server, attempt operation (verify `ConnectionError`), restart server, attempt operation (verify success).

### Layer 5: Parity tests

Run the same test sequence against both embedded mode and server mode. Assert that the results are structurally identical: same number of memories created, same recall results (modulo ordering when scores are equal), same error behavior. This ensures the two modes are behaviorally equivalent from the Python developer's perspective.

### Layer 6: Framework integration tests

- **LangChain:** Construct `HebbsMemory(hebbs=h)`, call `save_context({"input": "question"}, {"output": "answer"})`, call `load_memory_variables({"input": "question"})`, verify the answer appears in the result. Run against both modes.
- **CrewAI:** Construct `HebbsShortTermMemory(hebbs=h)`, verify basic store/retrieve lifecycle.

### Layer 7: Wheel validation tests

- For each platform wheel: install in a clean virtual environment, `import hebbs`, verify `HEBBS.open(tempdir)` works (native extension loaded).
- For the pure-Python fallback wheel: install, verify `import hebbs` works, verify `HEBBS("localhost:6380")` constructs without error, verify `HEBBS.open("./data")` raises `HebbsError` with actionable message.
- Verify `pip install hebbs[langchain]` installs `langchain-core` as a dependency and `from hebbs.integrations.langchain import HebbsMemory` succeeds.

---

## Deliverables Checklist

Phase 11 is done when ALL of the following are true:

**Package and build:**

- [ ] `hebbs` Python package exists in `crates/hebbs-python/` (maturin mixed layout: Rust source in `src/`, Python source in `hebbs/`)
- [ ] `pyproject.toml` declares maturin build backend, `requires-python = ">=3.9"`, minimal dependencies
- [ ] `maturin build --release` produces platform-specific wheels for Linux x86_64, Linux aarch64, macOS arm64, macOS x86_64
- [ ] Pure-Python fallback wheel (`py3-none-any`) is producible and supports server mode only
- [ ] Wheel size is < 50MB for each platform
- [ ] `pip install hebbs` in a clean virtualenv on a supported platform succeeds
- [ ] `import hebbs` succeeds on all supported platforms
- [ ] `py.typed` marker present for PEP 561 compliance

**Server mode:**

- [ ] `HEBBS("localhost:6380")` connects to a running `hebbs-server`
- [ ] `h.remember("text", importance=0.8)` returns a `Memory` dataclass
- [ ] `h.remember("text", importance=0.8, context={...}, entity_id="...", edges=[...])` supports full options
- [ ] `h.get(memory_id)` returns a `Memory` dataclass
- [ ] `h.recall("query")` returns a `RecallOutput` with `results: list[RecallResult]`
- [ ] `h.recall("query", strategy=RecallStrategy.TEMPORAL, entity_id="...")` supports all 4 strategies
- [ ] `h.recall("query", strategies=[...])` supports multi-strategy recall
- [ ] `h.prime(entity_id="...")` returns a `PrimeOutput`
- [ ] `h.revise(memory_id, content="new", importance=0.9)` returns updated `Memory`
- [ ] `h.forget(memory_id)` returns `ForgetOutput`
- [ ] `h.forget(entity_id="...", staleness_threshold_us=...)` supports criteria-based forget
- [ ] `h.subscribe(entity_id="...")` returns a `SubscribeStream` that yields `SubscribePush`
- [ ] `h.reflect(entity_id="...")` returns `ReflectOutput`
- [ ] `h.insights(entity_id="...", min_confidence=0.5)` returns `list[Memory]`
- [ ] `h.health()` returns `HealthStatus`
- [ ] Retry logic retries idempotent operations (get, recall, prime, insights) on transient errors
- [ ] Non-idempotent operations (remember, revise, forget, reflect) are never retried
- [ ] Timeout is enforced; raises `TimeoutError` with operation name and elapsed time

**Embedded mode:**

- [ ] `HEBBS.open("./path")` creates/opens a HEBBS engine in-process
- [ ] `HEBBS.open("./path", embedding_model="...", decay_half_life_days=14)` supports configuration
- [ ] All 9 operations work with the same method signatures as server mode
- [ ] GIL is released during all Rust engine operations (verified by concurrent thread test)
- [ ] Context manager works: `with HEBBS.open("./path") as h:` cleans up on exit
- [ ] `h.close()` explicitly shuts down the engine and releases the database lock
- [ ] Multiple `HEBBS.open()` calls to the same path raise `StorageError` (RocksDB lock)
- [ ] Native extension unavailability raises `HebbsError` with actionable message pointing to server mode

**Async API (server mode):**

- [ ] `from hebbs.aio import HEBBS` imports without error
- [ ] `h = await HEBBS.connect("localhost:6380")` connects
- [ ] `await h.remember(...)`, `await h.recall(...)`, etc. work as async methods
- [ ] `async for push in stream:` works for subscribe
- [ ] Async API runs on `asyncio` event loop without blocking

**Types and exceptions:**

- [ ] `Memory` is a Python dataclass with all fields typed and documented
- [ ] `memory.id` is a `str` (26-character ULID)
- [ ] `memory.context` is a `dict[str, Any]`
- [ ] `memory.kind` is a `MemoryKind` enum instance
- [ ] All enums (`MemoryKind`, `RecallStrategy`, `EdgeType`, `ContextMode`) are `enum.Enum` subclasses
- [ ] All exceptions are subclasses of `HebbsError`
- [ ] `MemoryNotFoundError` carries `.memory_id` attribute
- [ ] `TimeoutError` carries `.operation` and `.elapsed_seconds` attributes
- [ ] `InvalidInputError` carries `.field` and `.constraint` attributes
- [ ] Exception messages are actionable (a human reading the message knows what to do)

**Framework integrations:**

- [ ] `pip install hebbs[langchain]` installs langchain-core
- [ ] `from hebbs.integrations.langchain import HebbsMemory` imports
- [ ] LangChain `save_context` / `load_memory_variables` / `clear` lifecycle works
- [ ] `pip install hebbs[crewai]` installs crewai
- [ ] `from hebbs.integrations.crewai import HebbsShortTermMemory` imports
- [ ] CrewAI memory adapter basic lifecycle works

**Quality:**

- [ ] `mypy hebbs/` passes with zero errors (full type coverage)
- [ ] `ruff check hebbs/` passes with zero warnings
- [ ] `ruff format --check hebbs/` passes
- [ ] No `# type: ignore` comments without justification
- [ ] All public functions and classes have docstrings
- [ ] Parity test confirms identical behavior between server mode and embedded mode for all 9 operations

---

## Interfaces Published to Future Phases

Phase 11 creates contracts that later phases depend on. These interfaces are stable after Phase 11 and should not change without a documented migration plan.

| Interface | Consumer Phases | Stability Requirement |
|-----------|----------------|----------------------|
| `HEBBS` class constructor signatures (`HEBBS(endpoint)`, `HEBBS.open(path)`) | Phase 16 (documentation examples, getting started guide) | Stable. New optional parameters can be added. Constructor semantics never change. |
| Method signatures (`remember`, `recall`, `revise`, `forget`, `prime`, `subscribe`, `reflect`, `insights`) | Phase 16 (API reference), framework integrations | Method names and required parameter positions stable. New optional keyword arguments can be added. Return types stable (dataclass fields can be added, never removed). |
| Python type names (`Memory`, `RecallResult`, `RecallStrategy`, `MemoryKind`, etc.) | Phase 16 (documentation), framework integrations, user code | Class and enum names immutable. Field names immutable. New fields/variants can be added. |
| Exception class names and hierarchy | Phase 16, framework integrations, user code | Exception class names and inheritance immutable. New exception subclasses can be added. Attribute names on existing exceptions immutable. |
| Memory ID format (`str`, 26-character ULID) | Phase 16, framework integrations, user code | Memory IDs are `str` in Python. This matches the CLI output (Phase 9) and the Rust client SDK `Ulid.to_string()` format. |
| `pip install hebbs` package name on PyPI | Phase 16 (documentation), all external consumers | Package name immutable. Renaming a PyPI package is socially expensive. |
| Integration module paths (`hebbs.integrations.langchain`, `hebbs.integrations.crewai`) | Framework integration users, Phase 16 | Module paths stable. New integration modules can be added. |
| Optional extras names (`langchain`, `crewai`) | Phase 16 (documentation), framework users | Extra names stable. New extras can be added. |

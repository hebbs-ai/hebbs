# Phase 9: CLI Client -- Architecture Blueprint

## Status: ✅ COMPLETE

---

## Intent

Phases 1 through 8 built a complete cognitive memory engine and exposed it as a network service. But the only way to exercise that service is to write a gRPC client program or construct `curl` commands with JSON payloads. There is no fast, zero-ceremony way to test a single operation, inspect engine state, or debug a misbehaving subscription from the terminal.

Phase 9 fills this gap. It builds `hebbs-cli` -- the `redis-cli` of HEBBS. An interactive terminal client that connects to a running `hebbs-server` and exercises every operation with minimal friction. This is the tool that every developer will reach for before writing their first line of SDK code, and the tool that operators will use for diagnostics in production.

The decisions made here are load-bearing for three downstream concerns:

- **Phase 10 (Rust Client SDK):** The CLI is the first consumer of the tonic-generated gRPC client stubs from `hebbs-proto`. Pain points discovered here -- ergonomic gaps in the proto schema, awkward type conversions, missing server responses -- will be fixed before the Rust client SDK codifies them into a public API. The CLI is a proving ground.
- **Phase 12 (Testing and Benchmark Suite):** The CLI becomes the primary tool for manual acceptance testing and ad-hoc benchmarking. The integration test in Phase 12 will include a shell script that exercises the full lifecycle (remember, recall, revise, forget, reflect, insights) via `hebbs-cli` commands. The CLI's exit codes and JSON output format must be stable and machine-parseable for this.
- **Phase 15 (Deployment and Operations):** In production, operators use the CLI for health checks, diagnostics, and emergency operations (manual forget, manual reflect trigger, export for backup). The CLI must work correctly when pointed at a remote server across a network, not just localhost.

Unlike the previous phases which built internal infrastructure, Phase 9 produces the first user-facing tool. Every command name, every flag, every output format becomes part of the developer experience contract. Bad naming is permanent -- renaming `hebbs-cli recall` to `hebbs-cli search` after adoption is a breaking change that erodes trust. Design the command surface with the same care as the proto schema.

---

## Scope Boundaries

### What Phase 9 delivers

- `hebbs-cli` crate: a separate binary in the workspace, depending on `hebbs-proto` for gRPC client stubs and `clap` for argument parsing
- One-shot command mode: every one of the 9 HEBBS operations as a subcommand with flags, execute once and exit. `remember` and `revise` support `--edge TARGET_ID:EDGE_TYPE[:CONFIDENCE]` for graph edge creation.
- Interactive REPL mode: persistent connection, command history, tab completion, special dot-commands for session management
- Diagnostic commands beyond the 9 operations: `status`, `get`, `inspect`, `export`, `metrics`
- Three output formats: human-readable tables (default), JSON (`--json`), raw proto debug (`--raw`)
- Timing display: elapsed wall-clock time for every operation
- Colorized terminal output with tty detection and graceful degradation
- Meaningful exit codes for scripting integration
- Pipe-friendly stdin support for content ingestion
- Connection configuration via CLI flags, environment variables, and config file with clear precedence
- Verbose mode (`-v`, `-vv`) for gRPC request/response tracing

### What Phase 9 explicitly does NOT deliver

- Authentication headers (Phase 13 -- but the connection layer must accept an `--api-key` or `--token` flag without structural change)
- TLS/mTLS connection support (Phase 13 -- but the connection builder must accept `--tls-cert` and `--tls-key` flags that are currently no-ops with a clear "TLS not yet supported" message)
- Batch import command (Phase 12 -- the CLI handles one-at-a-time operations; bulk loading is a benchmark concern)
- Embedded mode (the CLI always connects to a server over gRPC; embedded mode lives in Phase 10's FFI layer)
- Shell auto-completion scripts (zsh/bash/fish completions are a polish item for Phase 15; `clap` can generate them, but distribution and installation are deployment concerns)
- Remote server discovery or service mesh integration (the user provides the endpoint explicitly)

These exclusions prevent scope creep. The CLI is a debugging and operations tool, not an SDK. It connects to a server and translates terminal input into gRPC calls. Nothing more.

---

## Architectural Decisions

### 1. Crate Position in the Dependency Graph

`hebbs-cli` is a binary crate that depends only on `hebbs-proto` from the HEBBS workspace. It does NOT depend on `hebbs-core`, `hebbs-storage`, `hebbs-embed`, `hebbs-index`, or `hebbs-reflect`. This is critical: the CLI is a thin network client, not an embedded engine. Its compile time must be a fraction of the server's. Its binary size must be a fraction of the server's (no RocksDB, no ONNX, no LLM HTTP clients).

Dependency set: `hebbs-proto` (gRPC stubs), `clap` (CLI parsing), `tonic` (gRPC client), `tokio` (async runtime), `serde_json` (JSON output), `hex` (ULID display), plus new crate-specific dependencies for REPL, table rendering, terminal colors, and time formatting. Detailed in the Dependencies section.

The strict dependency boundary means the CLI binary can be compiled and distributed independently. An operator can install the CLI on a jumpbox without needing the full engine toolchain.

### 2. Unified Command Dispatch Architecture

The CLI has two execution modes -- one-shot (subcommand from terminal arguments) and interactive (REPL loop). Both modes execute identical operations against the same server. The temptation is to implement each mode separately with shared utility functions. This leads to behavioral divergence: a flag that works in one-shot mode but is silently ignored in REPL mode.

**Decision: a unified command dispatch layer.**

Both modes produce the same internal `Command` enum. One-shot mode builds it from `clap` argument parsing against `std::env::args()`. REPL mode builds it from `clap` argument parsing against a tokenized input line (split on whitespace, respecting quoted strings). Both feed into the same async `execute(command, connection, output_config)` function that returns a typed result. Both pass that result to the same `render(result, format)` function that writes to stdout.

This means every command works identically in both modes. Testing one mode tests both. Adding a new command means adding one `clap` subcommand definition and one execution handler -- it automatically appears in both modes.

The key subtlety: `clap` is designed for `std::env::args()` parsing, but it also accepts `try_get_matches_from(iter)` which takes any iterator of strings. The REPL tokenizer splits the input line and feeds the tokens to the same clap `Command` definition with a synthetic program name prepended. If parsing fails, the error message comes from clap's own help system -- the user sees the same help text in both modes.

### 3. Async Runtime Strategy

The tonic gRPC client requires a tokio async runtime. The CLI is fundamentally a synchronous, single-user tool -- the user types a command, waits for the result, types the next command. The question is what shape the runtime takes.

**Three options considered:**

| Option | Runtime shape | Startup cost | Subscribe support | REPL compatibility |
|--------|-------------|-------------|-------------------|-------------------|
| A: Multi-threaded tokio | `tokio::runtime::Runtime::new()` with default thread pool | ~2ms, spawns N threads | Full (background receive while user waits) | Good (spawn tasks freely) |
| B: Current-thread tokio | `tokio::runtime::Builder::new_current_thread()` | ~0.1ms, single thread | Limited (must explicitly yield to receive pushes) | Requires careful task scheduling |
| C: Multi-threaded with restricted thread count | `tokio::runtime::Builder::new_multi_thread().worker_threads(2)` | ~1ms, spawns 2 threads | Good (one thread for gRPC I/O, one for user interaction) | Good |

**Decision: Option C (multi-threaded with 2 worker threads).**

Rationale: A single-threaded runtime (Option B) cannot simultaneously wait for REPL input on a blocking thread and receive subscribe stream pushes on the async runtime. The full default runtime (Option A) spawns threads equal to CPU cores, which is wasteful for a CLI. Two worker threads provide just enough concurrency: one handles the gRPC client I/O and subscribe stream receiving, while the other can service any secondary async tasks. The additional ~1ms startup cost is imperceptible.

**Implementation note:** The actual implementation runs the REPL loop as an async function on the tokio runtime. `rustyline::Editor::readline()` blocks the current thread, but since the tokio runtime has 2 worker threads, the other thread remains available for async I/O (gRPC calls, subscribe streaming). This simpler approach avoids the complexity of mpsc/oneshot channel communication between threads while still supporting concurrent operations like subscribe streaming. The tradeoff is that `readline()` blocks one worker thread during input, which is acceptable for a single-user CLI tool.

### 4. Connection Lifecycle Management

One-shot mode and REPL mode have fundamentally different connection needs.

**One-shot mode:** Connect, execute one RPC, disconnect. The connection is established lazily on first RPC call (tonic clients connect on first use, not on construction). The connection is dropped when the process exits. No reconnection logic needed -- if the server is unreachable, the CLI prints an error and exits with a non-zero code.

**REPL mode:** The connection persists for the session lifetime. The user expects to type 50 commands without re-authenticating or re-specifying the endpoint. Three concerns arise:

1. **Connection failure on startup:** If the server is unreachable when the REPL starts, the CLI should still enter the REPL (allowing the user to configure the endpoint via `.connect`). The first actual command will fail with a connection error and a helpful message.

2. **Mid-session disconnection:** If the server becomes unreachable mid-session (network partition, server restart), the next command fails. The CLI should detect this, print a message ("Connection lost to {endpoint}. Reconnecting on next command..."), and attempt to reconnect on the next command. No automatic background reconnection loop -- that adds complexity for a debugging tool.

3. **Server restart changes state:** If the server restarts (losing in-memory HNSW state until rebuild), the CLI does not need to detect this. The server's health endpoint handles readiness. Commands against an unready server return `UNAVAILABLE`, which the CLI renders as "Server is starting up. Try again in a moment."

**Decision:** Wrap the tonic client channels in a connection manager that lazily connects on first use, detects `UNAVAILABLE`/`UNKNOWN` status codes as potential disconnection signals, and retries connection establishment on the next command attempt. The REPL prompt indicates connection state (e.g., `hebbs(connected)>` vs `hebbs(disconnected)>`).

### 5. Memory ID Representation

Internally, memory IDs are 16-byte ULID binary. The proto schema uses `bytes` for memory IDs. The CLI must present and accept memory IDs in a human-friendly format.

ULIDs have a canonical 26-character Crockford Base32 encoding (e.g., `01HX3QZJV5FHKQ2MJ4DGRBMZK`). This encoding is case-insensitive, URL-safe, and sortable. It is the standard representation in every ULID library across languages.

**Decision:** The CLI displays all memory IDs as 26-character ULID strings. The CLI accepts memory IDs as either ULID strings (26 chars) or hex-encoded bytes (32 hex chars). The conversion between ULID string and 16-byte binary happens at the CLI boundary, inside the command dispatch layer, before the proto message is constructed.

This is the same decision that `redis-cli` makes for binary keys: display as a human-readable encoding, accept multiple input formats. The ULID string is the primary format because users will copy-paste from CLI output into subsequent commands.

### 6. Output Rendering System

The CLI supports three output formats, selected by flags that apply to all commands.

| Format | Flag | Use case | Renderer |
|--------|------|----------|----------|
| Human | (default, no flag) | Interactive terminal use | Tables, colors, relative timestamps, truncated content |
| JSON | `--json` | Scripting, piping to `jq`, integration tests | Machine-parseable JSON objects, one per line for streaming commands |
| Raw | `--raw` | Debugging proto serialization | Rust `Debug` format of the proto response message |

**Human format design decisions:**

- Memory content is truncated to fit terminal width. The full content is shown only in `get` (single memory detail view) or when `--full` is specified.
- Timestamps display as relative time ("2h ago", "3d ago") for recency context, with absolute ISO-8601 shown in verbose mode or `get` detail view.
- Importance scores display with a visual indicator: values above 0.8 render in bold or highlighted, values below 0.2 render dimmed. This gives an immediate visual signal for memory salience.
- Recall results display as a numbered list with relevance/confidence score, content preview, memory kind, and entity_id. This mirrors the standard search result format that developers expect.
- Every command prints elapsed wall-clock time on the final line (e.g., "3 results in 2.1ms"). This is unconditional -- it serves as both a performance indicator and a confirmation that the command completed.

**JSON format design decisions:**

- Every command produces a single JSON object on stdout (or one JSON object per line for streaming commands like `subscribe`).
- The JSON schema mirrors the proto response structure with field names in snake_case.
- Memory IDs are rendered as ULID strings (not hex, not base64) in JSON output. This matches what the human format shows, so a user can switch between formats without mental translation.
- Errors in JSON mode are also JSON objects with `error`, `code`, and `details` fields, written to stdout (not stderr) so that pipe consumers always get valid JSON.
- Timing information appears as a `_meta` field within the JSON object containing `elapsed_ms` and `server_address`.

**tty detection:** When stdout is a tty, the CLI uses colors and interactive formatting. When stdout is a pipe (detected via `std::io::IsTerminal` or the `atty` crate), colors are disabled, table borders are simplified, and output is optimized for machine consumption. The `--color` flag (`always`, `never`, `auto`) overrides detection.

### 7. REPL Architecture

The REPL is the flagship feature of `hebbs-cli`. It transforms the CLI from a one-shot tool into an interactive debugging environment. The design follows the `redis-cli` model that developers already understand.

**Entry condition:** Running `hebbs-cli` with no subcommand enters REPL mode. Running `hebbs-cli --endpoint host:port` with no subcommand enters REPL mode connected to the specified server. Running `hebbs-cli remember "some text"` (with a subcommand) runs in one-shot mode.

**REPL infrastructure:**

The `rustyline` crate provides readline-compatible line editing, command history, and a tab-completion hook. History is persisted to `~/.config/hebbs/cli_history` (on macOS/Linux) or the platform-appropriate config directory (via the `dirs` crate). History file is created on first REPL session.

Tab completion is registered as a `rustyline::Completer` implementation. It completes:
- Subcommand names at position 0 (e.g., typing `rec` + TAB completes to `recall`)
- Flag names after `--` (e.g., `--stra` + TAB completes to `--strategy`)
- Dot-command names after `.` (e.g., `.sta` + TAB completes to `.status`)
- Memory kind enum values where applicable (e.g., `--kind epi` + TAB completes to `episode`)
- Strategy names (e.g., `--strategy sim` + TAB completes to `similarity`)

Completion does NOT attempt to complete memory IDs, entity IDs, edge specifications, or free-text content -- these are dynamic values that the CLI does not cache.

**Dot-commands (REPL-only):**

Special commands prefixed with `.` that control the REPL session, not the HEBBS server. These never appear as one-shot subcommands because they are session-management concerns.

| Dot-command | Purpose |
|-------------|---------|
| `.help` | Print available commands and dot-commands |
| `.quit` / `.exit` | Exit the REPL (also Ctrl-D) |
| `.connect [endpoint]` | Connect or reconnect to a different server |
| `.disconnect` | Disconnect from the current server |
| `.status` | Show connection state, server version, memory count |
| `.clear` | Clear the terminal screen |
| `.history` | Show recent command history (not yet implemented -- rustyline v15 made history access private) |

**Prompt design:** The prompt reflects the connection state and the connected endpoint. Format: `hebbs {host}:{port}>` when connected, `hebbs (disconnected)>` when not connected. This gives the user constant awareness of which server they are talking to, which is critical when switching between development and production.

**REPL input tokenization:** The REPL line is tokenized into arguments using shell-like rules: whitespace-separated, double-quoted strings preserved as single arguments, backslash escaping within quotes. This is implemented via a simple state-machine tokenizer (not a shell parser -- no variable expansion, no pipes, no redirects). The tokenized arguments are fed to `clap::Command::try_get_matches_from()`.

### 8. Subscribe Streaming in the Terminal

Subscribe is the only streaming operation exposed via the CLI. It presents a unique UX challenge: the server sends memories asynchronously as they become relevant, but the terminal is a synchronous, line-oriented device.

**One-shot mode behavior:** `hebbs-cli subscribe --entity acme` starts a subscribe stream that runs until the user presses Ctrl-C (SIGINT). Each pushed memory prints as a formatted line. This is the behavior users expect from streaming CLI commands (like `tail -f`, `kubectl logs -f`, or `redis-cli SUBSCRIBE`). The `--json` flag produces one JSON object per line per push, suitable for piping into `jq` or a log aggregator.

**REPL mode behavior:** Typing `subscribe --entity acme` in the REPL enters "subscribe mode" -- the prompt changes to `hebbs subscribe>`, incoming pushes are printed as they arrive, and the user can type `feed "some text to match against"` to send text into the subscription. Ctrl-C (or typing `.stop`) exits subscribe mode and returns to the normal REPL prompt. While in subscribe mode, non-subscribe commands are not accepted.

This follows the `redis-cli SUBSCRIBE` precedent exactly: entering subscribe mode takes over the terminal, only subscribe-related actions are possible, and an explicit exit action returns to normal mode. This avoids the complexity of interleaving async push output with arbitrary command input.

**Feed mechanism:** In both modes, text can be fed to the subscription. In one-shot mode, `hebbs-cli feed --subscription-id <id> "some text"` sends text via the Feed RPC (the subscription must have been started separately, e.g., from a REPL or another terminal). In REPL subscribe mode, any input that is not a dot-command is treated as feed text.

### 9. Diagnostic Commands

Beyond the 9 HEBBS operations, the CLI provides diagnostic commands that compose existing RPCs for debugging convenience.

| Command | Underlying RPCs | Purpose |
|---------|----------------|---------|
| `status` | `HealthService.Check` | Server health, version, uptime, memory count, serving status |
| `get <id>` | `MemoryService.Get` | Full detail view of a single memory: all fields, all timestamps (absolute), embedding norm, decay score, access count |
| `inspect <id>` | `MemoryService.Get` + `MemoryService.Recall` (causal, depth 1) + `MemoryService.Recall` (similarity, top 5) | Composite view: the memory itself, its graph neighbors (edges in/out), and its nearest vector neighbors. This is the "explain this memory" command. |
| `export` | `MemoryService.Recall` (temporal, bounded) | Dump memories for an entity or globally, in JSONL format, one memory per line. Bounded by `--limit` (default 1000, max 10000). Not a streaming export -- for large datasets, use the SDK. |
| `metrics` | HTTP GET to `/v1/metrics` on the server's HTTP port | Fetch and display Prometheus metrics in a human-readable summary (not raw exposition format). Parses key metrics (operation latencies, memory count, error counts) into a table. |

**`inspect` design rationale:** This is the single most useful diagnostic command. When debugging "why didn't recall find this memory?", the developer needs to see: the memory's content and embedding, what other memories are connected to it (graph edges), and what its nearest neighbors are in vector space. This requires three RPCs composed client-side. A server-side Inspect RPC would be cleaner but violates Principle 10 (API Elegance -- the 9 operations are the API surface; diagnostic views are client concerns).

**`metrics` design decision:** The CLI connects to the server's HTTP port (not gRPC) for metrics. This means the CLI must know both the gRPC port (for operations) and the HTTP port (for metrics). The default convention is gRPC on 6380 and HTTP on 6381. The CLI accepts `--http-port` to override. If the HTTP port is unknown, `metrics` prints a helpful message explaining how to specify it.

### 10. Error Presentation and Exit Codes

gRPC errors arrive as `tonic::Status` with a code and message. The CLI must translate these into terminal-friendly output that helps the user fix the problem.

**Error rendering (human mode):**

| gRPC Status | CLI output |
|-------------|------------|
| `UNAVAILABLE` | "Error: Server unavailable at {endpoint}. Is hebbs-server running?" |
| `NOT_FOUND` | "Error: Memory {id} not found." |
| `INVALID_ARGUMENT` | "Error: {server message}" (the server already provides actionable messages) |
| `RESOURCE_EXHAUSTED` | "Error: {server message}" |
| `INTERNAL` | "Error: Server error -- {server message}. This may be a bug in HEBBS." |
| `DEADLINE_EXCEEDED` | "Error: Request timed out after {timeout}ms. Try increasing --timeout." |
| Connection refused | "Error: Cannot connect to {endpoint}. Is hebbs-server running on that address?" |

**Exit codes (one-shot mode only; REPL always exits 0 unless it crashes):**

| Code | Meaning | When |
|------|---------|------|
| 0 | Success | Command completed successfully |
| 1 | General error | Catch-all for unclassified errors |
| 2 | Usage error | Invalid arguments, missing required flags (from clap) |
| 3 | Connection error | Server unreachable, connection refused, TLS handshake failure |
| 4 | Not found | Requested resource (memory, subscription) does not exist |
| 5 | Server error | Server returned INTERNAL or UNAVAILABLE |

These exit codes are a contract for scripting. Phase 12 integration tests will assert on them. Changing them after adoption breaks shell scripts.

### 11. Configuration for the CLI

The CLI needs minimal configuration compared to the server. The primary concern is: where is the server?

**Configuration sources (highest priority first):**

1. CLI flags: `--endpoint`, `--timeout`, `--http-port`
2. Environment variables: `HEBBS_ENDPOINT`, `HEBBS_TIMEOUT`, `HEBBS_HTTP_PORT`
3. Config file: `~/.config/hebbs/cli.toml`
4. Compiled defaults: `localhost:6380`, timeout 30 seconds, HTTP port 6381

**Config file structure (minimal):**

The CLI config file is NOT the same as `hebbs.toml` (the server config). It lives in the user's home directory, not the project directory. It stores client-side preferences, not server configuration.

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `endpoint` | String | `localhost:6380` | gRPC server address |
| `http_port` | u16 | 6381 | HTTP server port (for metrics command) |
| `timeout_ms` | u64 | 30000 | Request timeout in milliseconds |
| `output_format` | String | `human` | Default output format (`human`, `json`, `raw`) |
| `color` | String | `auto` | Color mode (`always`, `never`, `auto`) |
| `history_file` | String | `~/.config/hebbs/cli_history` | REPL history file path |
| `max_history` | usize | 1000 | Maximum history entries to persist |

The config file is optional. The CLI works with zero configuration against a local server.

### 12. Pipe and Stdin Integration

The CLI must be pipe-friendly for shell scripting workflows.

**Stdin as content source:** When content is expected (e.g., `remember`), and stdin is not a tty, the CLI reads content from stdin. This enables patterns like piping text into remember.

**Precedence:** If `--content` flag is provided, it takes priority over stdin. If neither is provided and stdin is a tty, the CLI prints a usage error. If neither is provided and stdin is a pipe, the CLI reads stdin to EOF as the content.

**Stdout behavior when piped:** When stdout is a pipe, human formatting is simplified (no colors, no table borders, tab-separated columns instead of padded tables). JSON mode output is unchanged (it is already pipe-optimized).

**Stderr for diagnostics:** Timing information, verbose output (`-v`), and progress indicators go to stderr when stdout is a pipe. This keeps stdout clean for machine consumption.

---

## Dependencies

### New workspace dependencies to add

| Crate | Version | Purpose | Justification |
|-------|---------|---------|---------------|
| `rustyline` | 15 | Readline-compatible REPL with history and tab completion | The standard Rust crate for interactive CLI input. Used by ripgrep, delta, and other Rust CLI tools. Battle-tested, actively maintained. |
| `comfy-table` | 7 | Terminal table rendering | Lightweight, handles Unicode width, terminal width detection, and column wrapping. No heavy TUI framework dependency. |
| `owo-colors` | 4 | Terminal color output | Zero-allocation, faster than `colored`. Supports all standard terminal color codes. Does not allocate strings just to add color -- critical for high-throughput output like subscribe streaming. |
| `humantime` | -- | ~~Human-readable duration formatting~~ | **Not used.** Custom `format_relative_time` and `chrono_minimal` functions handle timestamp formatting without an external dependency, avoiding an extra crate for a few lines of logic. |
| `dirs` | 6 | Platform-specific config directory discovery | Returns `~/.config` on Linux, `~/Library/Application Support` on macOS, `%APPDATA%` on Windows. Used for REPL history file and CLI config file location. |

### Existing workspace dependencies used by hebbs-cli

| Crate | Already in workspace | Used for |
|-------|---------------------|----------|
| `hebbs-proto` | Yes | gRPC client stubs, proto message types |
| `clap` | Yes (4, derive) | CLI argument parsing, subcommands, help generation |
| `tonic` | Yes (0.12) | gRPC client |
| `tokio` | Yes (1, full) | Async runtime for tonic |
| `serde_json` | Yes (1) | JSON output formatting |
| `hex` | Yes (0.4) | Memory ID hex encoding (secondary format) |
| `tracing` | Yes (0.1) | Verbose mode structured logging |
| `tracing-subscriber` | Yes (0.3) | Subscriber for verbose mode log output |

### Dependencies NOT added (and why)

| Crate considered | Why rejected |
|-----------------|--------------|
| `crossterm` / `termion` | Full TUI framework -- far too heavy for a CLI that renders lines and tables. The `comfy-table` crate handles terminal width detection internally. |
| `indicatif` | Progress bars are unnecessary. The CLI operations are sub-second. Long operations (reflect, export) show a simple "waiting..." message on stderr. |
| `dialoguer` | Interactive prompts (confirm, select) are unnecessary. The CLI is command-driven, not wizard-driven. |
| `reqwest` / `hyper` (as HTTP client) | HTTP client for the metrics command. Not needed -- the `metrics` command makes a single HTTP GET via a raw `TcpStream` inside `spawn_blocking`. This avoids adding an HTTP client dependency for one endpoint. |

---

## Risk Register

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| REPL thread + async runtime interaction causes deadlocks or dropped commands | High -- CLI hangs, user loses trust in the tool | Medium (two-thread communication is subtle) | The REPL thread communicates with the async runtime via `tokio::sync::mpsc` (command channel) and `tokio::sync::oneshot` (result channel). The REPL thread blocks on the oneshot receiver with a timeout. If the timeout fires, the CLI prints "Command timed out" and the REPL continues. Integration tests exercise 100+ sequential commands in REPL mode to verify no leaks. |
| Subscribe mode output corrupts the terminal when pushes arrive rapidly | Medium -- garbled output, user confusion | Medium (rapid writes to stdout can interleave with prompt rendering) | Subscribe mode disables the rustyline prompt while active. Output is line-buffered. Each push is a single `writeln!` call. The REPL prompt is only re-rendered after subscribe mode exits. Terminal state is restored on Ctrl-C via a signal handler. |
| `clap` parsing of REPL input fails on edge cases (embedded quotes, special characters, unicode) | Low -- specific inputs produce parse errors | Medium (shell tokenization is deceptively complex) | The tokenizer handles double-quoted strings, single-quoted strings, and backslash escaping. Unicode within quotes is passed through unmodified. A property-based test generates random strings and verifies that tokenize-then-join is a round-trip. Edge cases in the tokenizer are bugs, not user errors. |
| Proto schema changes in later phases break the CLI | Medium -- CLI fails to compile or behaves incorrectly | Low (proto changes are additive per Phase 8 contract) | The CLI is in the same Cargo workspace as `hebbs-proto`. Any breaking proto change causes a compile error in the CLI, caught immediately. Additive changes (new fields, new RPCs) require no CLI updates -- unknown fields are ignored by default. New commands for new RPCs are added explicitly. |
| Large `export` output overwhelms terminal or crashes on memory | Medium -- CLI becomes unresponsive | Medium (user exports 10K+ memories without `--limit`) | The `export` command enforces a hard upper bound (10,000 memories). Output streams line-by-line (write each memory as it arrives, don't buffer the full result set). When stdout is a tty and the result exceeds terminal height, the last line shows "(showing {limit} of {total} memories, use --limit to adjust)". |
| rustyline version incompatibility or platform-specific bugs | Low -- REPL doesn't work on some platforms | Low (rustyline is mature and widely tested) | Pin `rustyline` to a specific stable major version. Test on macOS (primary development platform) and Linux (primary deployment platform). Windows support is not a Phase 9 target. |
| ULID parsing/formatting inconsistency between CLI and server | Medium -- memory IDs from CLI output don't work when pasted back as input | Low (ULID encoding is standardized) | The CLI uses the `ulid` crate (already in workspace) for encoding/decoding. A property-based test generates random ULIDs, encodes to string, decodes back, and verifies identity. The same crate is used by `hebbs-core` for generation, ensuring consistency. |
| HTTP metrics endpoint requires knowing a second port | Low -- `metrics` command fails with default config if HTTP port was changed | Medium (non-default configurations) | The CLI attempts the default HTTP port (6381). If the connection is refused, the error message explains: "Cannot reach HTTP metrics endpoint. Use --http-port to specify the server's HTTP port." The gRPC health check response (from `status` command) could include the HTTP port in future phases. |

---

## Testing Strategy

### Layer 1: Unit tests (in-crate)

- **Tokenizer:** Verify shell-like tokenization for: simple space-separated args, double-quoted strings, single-quoted strings, escaped quotes within strings, empty strings, unicode content, trailing whitespace, leading whitespace, multiple consecutive spaces.
- **ULID formatting:** Verify round-trip: random ULID bytes to string and back. Verify both ULID string format (26 chars) and hex format (32 chars) are accepted for memory IDs. Verify case-insensitive ULID parsing.
- **Output rendering:** Verify human table output for a known Memory proto message produces expected formatted output. Verify JSON output is valid parseable JSON. Verify raw output matches proto Debug format.
- **Timing display:** Verify elapsed time formatting for sub-millisecond, millisecond, second, and multi-second durations.
- **Error mapping:** For every gRPC status code, verify the CLI produces the expected error message and exit code.
- **Configuration loading:** Verify precedence: CLI flag overrides env var overrides config file overrides default. Verify missing config file results in defaults. Verify partial config file (only some keys) merges with defaults.
- **tty detection:** Verify that color and formatting are disabled when stdout is detected as a pipe (mock the detection for testing).

### Layer 2: Property-based tests

- **Tokenizer round-trip:** For any list of strings (the "args"), joining them with quoting and then tokenizing produces the original list.
- **ULID format round-trip:** For any 16-byte value, encoding as ULID string and decoding produces the original bytes.
- **JSON output validity:** For any proto Memory message with arbitrary field values, the JSON output mode produces valid JSON that `serde_json::from_str` can parse.
- **Exit codes are bounded:** For any error type the CLI can encounter, the exit code is in the range [0, 5].

### Layer 3: Integration tests

Integration tests start a real `hebbs-server` (with in-memory storage backend for speed), exercise CLI commands against it, and verify output and exit codes. These tests use `std::process::Command` to invoke the `hebbs-cli` binary.

- **Full lifecycle (one-shot mode):** Execute the following sequence as separate process invocations, each verifying exit code 0 and expected output:
  - `remember` with content and importance, capture memory ID from output
  - `get` with the captured ID, verify content matches
  - `recall` with similarity strategy, verify the remembered memory appears in results
  - `recall` with temporal strategy and entity_id
  - `revise` with the captured ID and new content, verify updated content
  - `recall` after revise, verify updated content appears
  - `forget` with the captured ID, verify success
  - `recall` after forget, verify empty results

- **JSON output round-trip:** Run `remember --json`, parse JSON output, extract memory ID. Run `get --json <id>`, parse JSON output, verify all fields present and correctly typed.

- **Error cases:** Run `get` with a non-existent memory ID, verify exit code 4 and error message. Run `remember` with empty content, verify exit code 2. Run with a wrong endpoint, verify exit code 3.

- **Pipe integration:** Echo content via stdin to `remember`, verify success. Pipe `recall --json` output to another process that counts results.

- **REPL smoke test:** Start `hebbs-cli` in REPL mode as a child process with pseudo-tty or piped stdin, send commands via stdin, read output, verify responses. This tests the basic REPL command dispatch (not the readline UX, which requires a real terminal).

- **Subscribe streaming:** Start a subscribe in one-shot mode in a background process, `remember` a matching memory in a separate invocation, verify the subscribe process prints the push, then send SIGINT to the subscribe process and verify clean exit.

- **Diagnostic commands:** Run `status` and verify it prints connection info. Run `metrics` and verify it connects to the HTTP port.

### Layer 4: Criterion benchmarks

CLI benchmarks are not performance-critical in the way engine benchmarks are. However, two measurements are valuable:

- **Command parse time:** Measure clap argument parsing time for the most complex subcommand (recall with multi-strategy). Target: < 100µs.
- **Output render time:** Measure table rendering time for 100 recall results in human format. Target: < 5ms.

These benchmarks primarily detect regressions from dependency upgrades (e.g., a new clap version that is significantly slower).

---

## Deliverables Checklist

Phase 9 is done when ALL of the following are true:

- [x] `hebbs-cli` crate exists in `crates/hebbs-cli/` and is a workspace member
- [x] `hebbs-cli` binary compiles as a separate binary from `hebbs-server`
- [x] `hebbs-cli` depends only on `hebbs-proto` from the HEBBS workspace (not `hebbs-core` or engine crates)
- [x] `hebbs-cli remember` sends a RememberRequest via gRPC and displays the created memory
- [x] `hebbs-cli remember --edge TARGET:TYPE[:CONFIDENCE]` attaches graph edges to the created memory (repeatable)
- [x] `hebbs-cli get <id>` retrieves a memory by ULID string and displays full details
- [x] `hebbs-cli recall` supports all four strategies (similarity, temporal, causal, analogical) via `--strategy` flag
- [x] `hebbs-cli recall --seed` and `--max-depth` flags support causal graph traversal
- [ ] `hebbs-cli recall` supports multi-strategy mode (only single strategy per invocation; proto supports multiple but CLI sends one)
- [x] `hebbs-cli revise <id>` sends a ReviseRequest and displays the updated memory
- [x] `hebbs-cli revise --edge TARGET:TYPE[:CONFIDENCE]` attaches graph edges on revision (repeatable)
- [x] `hebbs-cli forget` supports both single-ID and criteria-based deletion (--ids, --entity-id, --staleness-us, --access-floor, --kind, --decay-floor)
- [x] `hebbs-cli prime` sends a PrimeRequest and displays results
- [x] `hebbs-cli subscribe` starts a server-streaming subscription and prints pushes until Ctrl-C
- [x] `hebbs-cli feed` sends text to an active subscription via Feed RPC
- [x] `hebbs-cli reflect` triggers reflection and displays results (insight count, cluster count, memories processed)
- [x] `hebbs-cli insights` queries insights with optional filters (--entity-id, --min-confidence, --max-results)
- [x] `hebbs-cli status` displays server health, version, memory count, and uptime
- [x] `hebbs-cli inspect <id>` displays memory details, graph edges (causal depth 1), and nearest vector neighbors (top 5)
- [x] `hebbs-cli export` outputs memories in JSONL format with `--limit` bound (max 10,000)
- [x] `hebbs-cli metrics` fetches and displays Prometheus metrics in human-readable form via raw HTTP
- [x] `--format json` flag produces valid, parseable JSON for all commands
- [x] `--format raw` flag produces proto Debug output for all commands
- [x] Human output includes colorized display with tty detection and `--color always|never|auto` override
- [x] Human output includes elapsed time for every command (in human mode only)
- [x] Memory IDs display as 26-character ULID strings in all output formats
- [x] Memory IDs accept both ULID string (26 chars) and hex (32 chars) input formats
- [x] REPL mode activates when no subcommand is provided
- [x] REPL supports command history (persisted across sessions to `~/.config/hebbs/cli_history`)
- [x] REPL supports tab completion for subcommands, flags, strategy values, and kind values
- [x] REPL prompt displays connection state and endpoint
- [x] REPL dot-commands work: `.help`, `.quit`, `.connect`, `.disconnect`, `.status`, `.clear`
- [ ] REPL subscribe mode: entering `subscribe` works in REPL but does not yet have a dedicated `.stop` command (Ctrl-C exits the entire REPL)
- [x] Pipe-friendly: content can be piped via stdin to `remember`
- [x] Pipe-friendly: colors disabled when stdout is not a tty; timing goes to stderr when piped
- [x] Exit codes are correct: 0 (success), 1 (general), 2 (usage error), 3 (connection error), 4 (not found), 5 (server error)
- [x] Configuration loads from `--endpoint` flag, `HEBBS_ENDPOINT` env var, `~/.config/hebbs/cli.toml`, and defaults
- [x] Verbose mode (`-v`) enables debug tracing to stderr; `-vv` enables trace level
- [x] No `unwrap()` or `expect()` on any path reachable by external input
- [x] No `unsafe` blocks
- [x] All unit tests pass (50 unit tests across config, connection, format, tokenizer modules)
- [ ] Integration tests (full lifecycle via CLI against a running server) deferred to Phase 12
- [x] Property-based tests pass for tokenizer, ULID formatting, JSON output, and renderer (15 proptest cases)
- [x] CLI parsing tests pass for all subcommands and global flags (32 tests)
- [x] Error tests pass for all CliError variants and exit codes (14 tests)
- [x] Renderer tests pass for all output formats and data types (19 tests)
- [x] `cargo clippy` passes with zero warnings on `hebbs-cli`
- [ ] `cargo audit` passes (not yet run in CI)
- [x] PhasePlan.md updated with Phase 9 completion marker
- [x] DocsSummary.md updated with Phase 9 entry

### Edge Specification Format

The `--edge` flag (on `remember` and `revise`) accepts the format `TARGET_ID:EDGE_TYPE[:CONFIDENCE]`:

- **TARGET_ID**: 26-char ULID string or 32-char hex string identifying the target memory
- **EDGE_TYPE**: One of `caused_by`, `related_to`, `followed_by`, `revised_from`, `insight_from`
- **CONFIDENCE**: Optional float 0.0-1.0

The flag is repeatable for multiple edges:

```
hebbs-cli remember "Customer called about invoice" \
  --edge 01HX3QZJV5FHKQ2MJ4DGRBMZK:caused_by:0.9 \
  --edge 01HX3QZJV5FHKQ2MJ4DGRBMZL:related_to
```

This enables full causal graph construction from the CLI, making `recall --strategy causal` fully functional.

---

## Interfaces Published to Future Phases

Phase 9 creates contracts that later phases depend on. These interfaces are stable after Phase 9 and should not change without a documented migration plan.

| Interface | Consumer Phases | Stability Requirement |
|-----------|----------------|----------------------|
| CLI subcommand names and flag names | Phase 12 (integration test scripts reference CLI commands), Phase 15 (runbook references CLI commands for operations), Phase 16 (documentation site documents CLI usage) | Subcommand names are immutable. Flag names are immutable. New subcommands and flags can be added. Existing ones never removed or renamed. |
| Exit codes (0, 2, 3, 4, 5) | Phase 12 (integration test scripts assert on exit codes), Phase 15 (monitoring scripts may check exit codes) | Exit code values and their meanings are immutable after Phase 9. |
| JSON output schema | Phase 12 (test scripts parse JSON output), Phase 15 (operations tooling may parse CLI output) | JSON field names are stable. New fields can be added. Existing fields never removed or renamed. Memory IDs are always ULID strings in JSON. |
| REPL dot-command names | Phase 16 (documentation) | Dot-command names are stable. New dot-commands can be added. |
| Config file location and format (`~/.config/hebbs/cli.toml`) | Phase 15 (deployment guides reference config), Phase 16 (documentation) | File location convention and key names are stable after Phase 9. New keys can be added. |
| ULID string format for memory ID display | Phase 10 (Rust client SDK should use the same display format), Phase 11 (Python SDK should use the same display format), Phase 18-19 (TypeScript/Go SDKs) | The 26-character Crockford Base32 ULID string is the canonical human-readable memory ID format across all client interfaces. |

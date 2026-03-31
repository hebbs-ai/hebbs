# HEBBS Demo -- Developer Setup

> **Audience:** Developers with the full `hebbs-repos/` workspace (all repos checked out locally).
> This doc tests the install script, server, and demo **from local files** so you can
> validate everything works before pushing to GitHub and deploying the website.
>
> For the public-facing install (users who only have the `hebbs` repo), see
> [`hebbs/README.md`](../hebbs/README.md) which uses `curl -sSf https://hebbs.ai/install | sh`.

## Prerequisites

- Python >= 3.10
- macOS (Apple Silicon) or Linux (x86\_64 / aarch64)
- Rust toolchain (only if GitHub releases don't exist yet for your version)
- The `hebbs-repos/` workspace with at least `hebbs/`, `hebbs-python/`, and `hebbs-website/` cloned

## 1. Install the HEBBS Server

<!-- NOTE FOR AI AGENTS AND FUTURE EDITORS:
     DO NOT change any install command here to `curl -sSf https://hebbs.ai/install | sh`.
     This document is for developers with the full hebbs-repos workspace.
     The install script is run from the LOCAL filesystem to validate it works
     BEFORE it is deployed to hebbs.ai. The website URL belongs in hebbs/README.md,
     not here.
-->

### Build from Source

There are no published GitHub releases yet. Build the server from the local Rust source:

```bash
cd ../hebbs
cargo build --release -p hebbs-server -p hebbs-cli
```

Copy the binaries to `~/.hebbs/bin/` (the same location the install script would use):

```bash
mkdir -p ~/.hebbs/bin
cp target/release/hebbs-server ~/.hebbs/bin/
cp target/release/hebbs-cli ~/.hebbs/bin/
export PATH="$HOME/.hebbs/bin:$PATH"
```

> **macOS note:** If you built inside Cursor (or another sandboxed IDE), macOS may
> kill the binary on launch (`zsh: killed`). Fix by re-signing:
>
> ```bash
> codesign --force --sign - ~/.hebbs/bin/hebbs-server
> codesign --force --sign - ~/.hebbs/bin/hebbs-cli
> ```

### Validate the Install Script (once releases are published)

After you create a GitHub release and push the website, validate the install script
works end-to-end by running it from the local filesystem:

```bash
sh ../hebbs-website/public/install
```

This runs the exact same script that production users get via `curl -sSf https://hebbs.ai/install | sh`.
It detects your platform, downloads the release tarball from GitHub, verifies the checksum,
and installs `hebbs-server`, `hebbs-cli`, and `hebbs-bench` into `~/.hebbs/bin/`.

You can override variables:

```bash
HEBBS_VERSION=v0.1.0 sh ../hebbs-website/public/install
HEBBS_INSTALL_DIR=/usr/local/bin sh ../hebbs-website/public/install
HEBBS_NO_VERIFY=1 sh ../hebbs-website/public/install
```

### Verify

```bash
hebbs-server --version
hebbs-cli --version
```

## 2. Start the HEBBS Server

```bash
HEBBS_AUTH_ENABLED=false hebbs-server
```

The server starts on `localhost:6380` (gRPC) and `localhost:6381` (HTTP) with defaults:
- Embedding: BGE-small-en-v1.5 via ONNX (downloads on first run)
- Storage: `./hebbs-data/` (RocksDB)
- Auth: disabled for local dev (set `HEBBS_AUTH_ENABLED=true` for production)
- Reflect: uses OpenAI GPT-4o by default (set `OPENAI_API_KEY` for real insights)

To use real LLM-powered reflection (insight generation from memory clusters):

```bash
export OPENAI_API_KEY="your-key"
HEBBS_AUTH_ENABLED=false hebbs-server
```

Without `OPENAI_API_KEY`, the reflect pipeline falls back to mock mode.

Verify it works:

```bash
hebbs-cli remember "hello world"
hebbs-cli recall "hello"
```

## 3. Install the Python Package

From the `hebbs-python/` directory:

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -e ".[demo,dev]"
```

This installs:
- `hebbs` -- Python SDK (async gRPC client)
- `hebbs-demo` -- CLI demo app (Rich panels, 5 LLM providers, 7 scripted scenarios)
- Dev tools (pytest, ruff, mypy, grpcio-tools)

## 4. Configure the LLM

The demo app uses an LLM for conversation generation and memory extraction. Choose one:

### Gemini via Vertex AI

Requires a GCP service account with Vertex AI access.

```bash
export GOOGLE_APPLICATION_CREDENTIALS="/path/to/service-account-key.json"
export GOOGLE_CLOUD_PROJECT="your-gcp-project-id"
export GOOGLE_CLOUD_LOCATION="asia-south1"
```

### Gemini with API Key

```bash
export GEMINI_API_KEY="your-gemini-api-key"
```

### OpenAI

```bash
export OPENAI_API_KEY="your-openai-api-key"
```

### Anthropic

```bash
export ANTHROPIC_API_KEY="your-anthropic-api-key"
```

### Mock LLM (no keys needed)

No setup required. Use `--mock-llm` on any command.

### Ollama (local, free)

```bash
ollama pull llama3.2
```

## 5. Run the Demo

### Interactive Chat

```bash
# Gemini Vertex AI (best quality for demos)
hebbs-demo interactive --config gemini-vertex --verbosity verbose

# Gemini API key
hebbs-demo interactive --config gemini

# OpenAI
hebbs-demo interactive --config openai

# Ollama (local)
hebbs-demo interactive --config local

# Mock LLM (no API keys, deterministic)
hebbs-demo interactive --mock-llm

# Custom entity name
hebbs-demo interactive --config gemini-vertex --entity-id acme_corp
```

### In-Session Commands

| Command | Description |
|---------|-------------|
| `/memories` | Show all stored memories for the current entity |
| `/recall <query>` | Manually query HEBBS recall with a cue |
| `/brain` | Show engine state: memory count, entity, config |
| `/stats` | Show HEBBS engine latency + LLM token usage and cost |
| `/reflect` | Trigger HEBBS reflect (generate insights from clusters) |
| `/forget [entity]` | GDPR-forget all memories for an entity |
| `/insights` | Show accumulated insights for this entity |
| `/session <entity>` | Switch to a different prospect entity |
| `/count` | Total memory count across all entities |
| `/help` | Show all commands |
| `quit` | Exit (prints session summary: LLM vs HEBBS latency, cost) |

### Scenarios

Run the 7 scripted validation scenarios:

```bash
# All scenarios with mock LLM (fast, no API keys)
hebbs-demo scenarios --all

# All scenarios with real LLM
hebbs-demo scenarios --all --real-llm --config gemini-vertex

# Single scenario
hebbs-demo scenarios --run discovery_call

# Verbose output showing all HEBBS operations
hebbs-demo scenarios --all --verbosity verbose
```

| # | Scenario | What It Validates |
|---|----------|-------------------|
| 1 | `discovery_call` | Single-session memory formation and similarity recall |
| 2 | `objection_handling` | Cross-entity analogical recall for pricing objections |
| 3 | `multi_session` | Five-session relationship with temporal recall and prime |
| 4 | `reflect_learning` | Bulk ingest 50 memories, run reflect, verify institutional learning |
| 5 | `subscribe_realtime` | Seed memories, open subscribe stream, feed text, validate surfacing |
| 6 | `forget_gdpr` | Store 20 memories, execute entity forget, confirm GDPR erasure |
| 7 | `multi_entity` | Interleave memories across three entities, validate isolation |

Expected output on a clean server:

```
7/7 scenarios passed
45/45 assertions passed
Total wall time: 0.8s
```

## 6. Entity Isolation & Multitenancy

All HEBBS operations are scoped by `entity_id`. Data stored under one entity is never visible when querying a different entity. This applies to all recall strategies (similarity, temporal, causal, analogical), prime, reflect, and insights.

Use `/session <entity>` to switch entities mid-conversation:

```
You: /session acme_corp
# Prime loads only acme_corp memories
You: Tell me about their CRM
# Recall only matches acme_corp data

You: /session techflow_inc
# Prime loads zero memories (new entity, no data)
# Agent has no knowledge of acme_corp conversations
```

The session summary at exit and the `/stats` command show HEBBS engine latency alongside LLM latency, making it easy to see sub-10ms engine performance vs multi-second LLM calls:

```
│ Total LLM latency  │                       28,039ms │
│ HEBBS remember     │      4.5ms avg (18ms total)    │
│ HEBBS recall       │      5.5ms avg (22ms total)    │
│ HEBBS prime        │      3.2ms avg (3ms total)     │
```

## 7. Config Files

Pre-built configs live in `demo/configs/`:

| File | LLM Provider | Env Var Required |
|------|-------------|------------------|
| `gemini-vertex.toml` | Gemini 2.5 Flash via Vertex AI | `GOOGLE_CLOUD_PROJECT`, `GOOGLE_APPLICATION_CREDENTIALS` |
| `gemini.toml` | Gemini 2.0 Flash via API key | `GEMINI_API_KEY` |
| `openai.toml` | GPT-4o / GPT-4o-mini | `OPENAI_API_KEY` |
| `local.toml` | Ollama llama3.2 (local) | None (Ollama must be running) |

Custom configs: copy any TOML and edit the `[llm]` section. Pass with `--config path/to/your.toml`.

## 8. Development Workflow

### Regenerate Proto Stubs

If `proto/hebbs.proto` changes:

```bash
sh scripts/generate_proto.sh
```

### Run Tests

```bash
pytest
pytest -m "not requires_server"
```

### Lint and Type Check

```bash
ruff check src/ demo/ tests/
mypy src/hebbs/
```

## 9. Project Layout

```
hebbs-python/
  src/hebbs/              SDK (pip-installable as "hebbs")
    client.py             HebbsClient -- async gRPC client
    types.py              Memory, RecallResult, etc. (pure dataclasses)
    exceptions.py         HebbsError hierarchy
    services/             Per-service gRPC wrappers (memory, health, reflect, subscribe)
    _generated/           Generated protobuf stubs (committed)
  demo/                   Demo app (pip-installable as "demo")
    cli.py                Click CLI entry point + session summary with HEBBS/LLM latency
    agent.py              SalesAgent conversation loop + HebbsSessionStats
    memory_manager.py     Memory extraction + HEBBS calls + latency tracking
    display.py            Rich terminal panels (REMEMBER, RECALL, PRIME, REFLECT)
    llm_client.py         Gemini / OpenAI / Anthropic / Ollama
    config.py             TOML config loader
    prompts.py            LLM prompt templates
    configs/              TOML config files per provider
    scenarios/            7 scripted validation scenarios
  proto/                  Source proto (copied from hebbs/proto/)
  scripts/                Proto codegen script
  tests/                  Unit tests
  pyproject.toml          Build config + dependencies
```

## Troubleshooting

### Install script fails with 404

GitHub releases don't exist yet for the requested version. Use the "Build from Source" fallback in Step 1.

### "Failed to connect to HEBBS server"

The server is not running or not on the expected port.

```bash
HEBBS_AUTH_ENABLED=false hebbs-server &
hebbs-cli remember "test" && hebbs-cli recall "test"
```

### "missing authorization metadata"

The server has auth enabled. Restart with auth disabled for local dev:

```bash
HEBBS_AUTH_ENABLED=false hebbs-server
```

### "Address already in use (os error 48)"

Another process holds port 6380. Kill it and restart:

```bash
lsof -ti :6380 | xargs kill -9
sleep 2
HEBBS_AUTH_ENABLED=false hebbs-server
```

### "GEMINI_API_KEY not set" / API key warnings

Set the required env var for your chosen provider, or skip with `--mock-llm`:

```bash
hebbs-demo interactive --mock-llm
```

### Gemini model not available in your region

`gemini-2.5-flash` is available in `asia-south1` and `us-central1`. Update `GOOGLE_CLOUD_LOCATION` or the `location` field in your config TOML.

### Proto stubs out of date

```bash
sh scripts/generate_proto.sh
```

### Python version too old

The SDK requires Python 3.10+:

```bash
python3 --version
```

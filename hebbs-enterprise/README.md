# HEBBS Enterprise

Cognitive memory engine for AI agents. Deploy on your infrastructure in one line.

HEBBS ships as a skill for Claude Code and OpenClaw. No SDK integration. No glue code. No configuration. Install HEBBS, and your agent automatically stores memories, recalls with the right strategy, consolidates insights, and forgets what's stale.

## Quick Start

```sh
curl -sSf https://hebbs.ai/server | OPENAI_API_KEY=sk-your-key sh
```

Open `http://your-server:8080` when it finishes. The onboarding wizard runs on first visit: create your admin account, name your first workspace, and save the generated API key.

## CLI (for your developers)

```sh
curl -sSf https://hebbs.ai/install | sh
hebbs login --endpoint http://your-server:8080 --api-key <your-workspace-key>
```

```sh
hebbs recall "your query"
hebbs remember "important fact"
hebbs push ./docs
```

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `OPENAI_API_KEY` | (required) | Your OpenAI API key |
| `HEBBS_PORT` | 8080 | Dashboard + API port |
| `HEBBS_LLM_MODEL` | gpt-4o-mini | LLM model for extraction |
| `HEBBS_EMBED_MODEL` | text-embedding-3-small | Embedding model |
| `HEBBS_DIR` | ./hebbs | Installation directory |
| `HEBBS_VERSION` | 0.4.0 | Image version |

## Operations

```sh
cd hebbs/                                           # your install directory
docker compose logs -f                              # view logs
docker compose down                                 # stop
docker compose pull && docker compose up -d         # upgrade
```

## Support

https://hebbs.ai

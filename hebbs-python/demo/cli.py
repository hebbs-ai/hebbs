"""Click CLI: interactive, scenarios.

Entry point for the hebbs-demo command.
Connects to a running HEBBS server via gRPC.
"""

from __future__ import annotations

import asyncio
import sys
from pathlib import Path

import click
from rich.console import Console
from rich.panel import Panel
from rich.table import Table
from rich.text import Text

from demo.config import DemoConfig
from demo.display import DisplayManager, Verbosity

console = Console()

VERBOSITY_MAP = {
    "quiet": Verbosity.QUIET,
    "normal": Verbosity.NORMAL,
    "verbose": Verbosity.VERBOSE,
}


def _resolve_config(config_path: str | None) -> DemoConfig:
    if config_path:
        p = Path(config_path)
        if not p.exists():
            configs_dir = Path(__file__).parent / "configs"
            candidate = configs_dir / f"{config_path}.toml"
            if candidate.exists():
                return DemoConfig.from_toml(candidate)
        return DemoConfig.from_toml(p)
    default_path = Path(__file__).parent / "configs" / "gemini.toml"
    if default_path.exists():
        return DemoConfig.from_toml(default_path)
    return DemoConfig.default()


async def _connect_hebbs(config: DemoConfig):
    from hebbs import HebbsClient
    client = HebbsClient(config.hebbs.address)
    await client.connect()
    return client


_WELCOME_BANNER = """\
[bold cyan]
   +===========================================================+
   |          HEBBS Demo -- Meet "Atlas"                        |
   |          Your AI Sales Agent for HEBBS                     |
   +===========================================================+[/bold cyan]

[dim]Atlas sells HEBBS -- a cognitive memory engine for AI applications.
You are the prospect. Every message is embedded, recalled, and
remembered -- watch the engine work in real time.[/dim]
"""

_GUIDED_TOPICS = """\
[bold]Try these to see different HEBBS recall strategies in action:[/bold]

  [cyan]1.[/cyan] "Tell me about HEBBS"                     [dim]-> similarity recall[/dim]
  [cyan]2.[/cyan] "What have we discussed so far?"           [dim]-> temporal recall[/dim]
  [cyan]3.[/cyan] "What led you to that recommendation?"     [dim]-> causal recall[/dim]
  [cyan]4.[/cyan] "Any companies with similar needs?"        [dim]-> analogical recall[/dim]
  [cyan]5.[/cyan] Type [cyan]/reflect[/cyan] then "What patterns have you learned?"

[dim]Or just chat naturally -- all four strategies run on every turn.[/dim]"""


_HELP_TEXT = """\
[bold]Conversation[/bold]
  Just type naturally -- you are the sales prospect, Atlas responds.

[bold]Inspect HEBBS Brain[/bold]
  [cyan]/memories[/cyan]          Show all stored memories for this entity
  [cyan]/recall[/cyan] <query>    Manually query HEBBS recall with a cue
  [cyan]/brain[/cyan]             Show engine state: memory count, entity, config
  [cyan]/stats[/cyan]             Show HEBBS engine + LLM usage stats

[bold]Engine Operations[/bold]
  [cyan]/reflect[/cyan]           Trigger HEBBS reflect (generate insights from clusters)
  [cyan]/forget[/cyan] [entity]   GDPR-forget all memories for an entity
  [cyan]/insights[/cyan]          Show accumulated insights for this entity
  [cyan]/count[/cyan]             Total memory count across all entities

[bold]Session[/bold]
  [cyan]/session[/cyan] <entity>  Switch to a different prospect entity
  [cyan]/help[/cyan]              Show this help
  [cyan]quit[/cyan]               Exit"""


@click.group()
@click.version_option(version="0.3.0", prog_name="hebbs-demo")
def main():
    """HEBBS Demo: AI Sales Intelligence Agent (gRPC client)."""
    pass


@main.command()
@click.option("--config", "config_path", default=None, help="Config name or path to TOML file")
@click.option(
    "--verbosity", type=click.Choice(["quiet", "normal", "verbose"]),
    default="verbose", help="Display verbosity level",
)
@click.option("--mock-llm", is_flag=True, help="Use mock LLM (no API keys needed)")
@click.option("--entity-id", default="prospect", help="Entity ID for the conversation")
def interactive(config_path: str | None, verbosity: str, mock_llm: bool, entity_id: str):
    """Start an interactive conversation with the AI sales agent."""
    asyncio.run(_interactive_async(config_path, verbosity, mock_llm, entity_id))


async def _interactive_async(
    config_path: str | None, verbosity: str, mock_llm: bool, entity_id: str,
):
    from demo.agent import SalesAgent

    entity = entity_id  # short alias for internal use

    cfg = _resolve_config(config_path)
    warnings = cfg.validate()
    if warnings and not mock_llm:
        for w in warnings:
            console.print(f"[yellow]Warning:[/yellow] {w}")
        console.print("[dim]Use --mock-llm to run without API keys[/dim]")
        console.print()

    display = DisplayManager(VERBOSITY_MAP[verbosity], console)
    console.print(_WELCOME_BANNER)

    try:
        hebbs = await _connect_hebbs(cfg)
    except Exception as e:
        console.print(f"[red]Failed to connect to HEBBS server at {cfg.hebbs.address}:[/red] {e}")
        console.print("[dim]Make sure hebbs-server is running. Install with: curl -sSf https://hebbs.ai/install | sh[/dim]")
        sys.exit(1)

    try:
        agent = SalesAgent(
            config=cfg,
            hebbs=hebbs,
            display=display,
            use_mock_llm=mock_llm,
        )

        llm_label = "[yellow]mock[/yellow]" if mock_llm else f"[green]{cfg.llm.conversation_provider}/{cfg.llm.conversation_model}[/green]"
        console.print(f"  LLM:       {llm_label}")
        console.print(f"  Server:    [green]{cfg.hebbs.address}[/green]")
        console.print(f"  Entity:    [bold]{entity}[/bold]")
        console.print(f"  Verbosity: {verbosity}")
        console.print()

        await agent.start_session(entity_id=entity, session_num=1)

        console.print()
        console.print(Panel(_GUIDED_TOPICS, title="Getting Started", border_style="green"))
        console.print()
        console.print("[dim]Type /help for all commands.[/dim]")
        console.print()

        while True:
            try:
                user_input = await asyncio.to_thread(
                    lambda: console.input("[bold blue]You:[/bold blue] ").strip()
                )
            except (EOFError, KeyboardInterrupt):
                console.print("\n[dim]Goodbye![/dim]")
                break

            if not user_input:
                continue

            if user_input.lower() in ("quit", "exit", "q"):
                console.print("[dim]Goodbye![/dim]")
                break

            if user_input.startswith("/"):
                await _handle_command(
                    user_input, agent, hebbs, entity, console,
                    mock_llm=mock_llm, cfg=cfg,
                )
                continue

            await agent.process_turn(
                prospect_message=user_input,
                recall_strategies=["similarity", "temporal", "causal", "analogical"],
            )

    finally:
        await agent.end_session()
        _print_session_metrics(agent, cfg, mock_llm, console)
        await hebbs.close()


def _print_session_metrics(agent, cfg: DemoConfig, mock_llm: bool, console: Console) -> None:
    """Print a summary metrics panel at the end of an interactive session."""
    hs = agent.hebbs_stats
    llm_stats = agent.llm_client.stats
    llm_label = "mock" if mock_llm else f"{cfg.llm.conversation_provider}/{cfg.llm.conversation_model}"

    table = Table(title="Session Summary", show_header=True, header_style="bold cyan")
    table.add_column("Metric", style="white")
    table.add_column("Value", justify="right", style="cyan")

    mm = agent.memory_manager

    table.add_row("LLM provider", llm_label)
    table.add_row("Conversation turns", str(hs.turns))
    table.add_row("Memories created", str(hs.memories_created))
    table.add_row("Memories recalled", str(hs.memories_recalled))
    table.add_row("Total LLM calls", str(llm_stats.total_calls))
    table.add_row("Input tokens", f"{llm_stats.total_input_tokens:,}")
    table.add_row("Output tokens", f"{llm_stats.total_output_tokens:,}")
    if llm_stats.total_calls > 0:
        table.add_row("Avg latency/call", f"{llm_stats.total_latency_ms / llm_stats.total_calls:,.0f}ms")
    table.add_row("Total LLM latency", f"{llm_stats.total_latency_ms:,.0f}ms")
    if mm.remember_batches > 0:
        table.add_row("HEBBS remember", f"{mm.total_remember_ms / mm.remember_batches:,.1f}ms avg ({mm.total_remember_ms:,.0f}ms total)")
    else:
        table.add_row("HEBBS remember", "—")
    if mm.recall_batches > 0:
        table.add_row("HEBBS recall", f"{mm.total_recall_ms / mm.recall_batches:,.1f}ms avg ({mm.total_recall_ms:,.0f}ms total)")
    else:
        table.add_row("HEBBS recall", "—")
    if mm.prime_calls > 0:
        table.add_row("HEBBS prime", f"{mm.total_prime_ms / mm.prime_calls:,.1f}ms avg ({mm.total_prime_ms:,.0f}ms total)")
    else:
        table.add_row("HEBBS prime", "—")
    table.add_row("Est. cost", f"${llm_stats.estimated_cost_usd:.4f}")
    console.print()
    console.print(table)


async def _handle_command(
    cmd: str, agent, hebbs, entity: str, console: Console,
    *, mock_llm: bool = False, cfg: DemoConfig | None = None,
):
    parts = cmd.strip().split(maxsplit=1)
    command = parts[0].lower()

    if command == "/memories":
        try:
            prime_out = await hebbs.prime(entity_id=entity, max_memories=100)
            memories = prime_out.results
            if not memories:
                console.print(Panel(
                    "[dim]No memories stored yet for this entity.[/dim]",
                    title=f"HEBBS Brain -- {entity}",
                    border_style="cyan",
                ))
                return
            table = Table(
                title=f'Stored Memories for "{entity}" ({len(memories)} total)',
                show_header=True, header_style="bold cyan",
                show_lines=True, expand=True,
            )
            table.add_column("#", style="dim", width=3, justify="right")
            table.add_column("Content", style="white", ratio=4)
            table.add_column("Imp.", justify="center", width=5)
            table.add_column("Context", style="dim", ratio=2)
            table.add_column("Kind", justify="center", width=8)
            for i, r in enumerate(memories, 1):
                mem = r.memory
                kind = mem.kind.value
                imp = f"{mem.importance:.1f}"
                ctx_parts = []
                if mem.context:
                    for k, v in list(mem.context.items())[:4]:
                        ctx_parts.append(f"{k}={v}")
                ctx_str = ", ".join(ctx_parts) if ctx_parts else "-"
                kind_style = "cyan" if kind == "insight" else "white"
                table.add_row(
                    str(i), mem.content, imp, ctx_str,
                    Text(kind.title(), style=kind_style),
                )
            console.print(table)
        except Exception as e:
            console.print(f"[red]Error listing memories:[/red] {e}")

    elif command == "/recall":
        query = parts[1] if len(parts) > 1 else ""
        if not query:
            console.print("[dim]Usage: /recall <your query text>[/dim]")
            return
        try:
            recall_out = await hebbs.recall(
                cue=query,
                strategies=["similarity", "temporal", "analogical"],
                top_k=5,
                entity_id=entity,
            )
            results = recall_out.results
            if results:
                lines = []
                for r in results:
                    mem = r.memory
                    score = f"{r.score:.3f}"
                    strats = ", ".join(
                        d.strategy for d in r.strategy_details
                    ) if r.strategy_details else "?"
                    lines.append(f"  [{score}] ({strats}) {mem.content}")
                console.print(Panel(
                    "\n".join(lines),
                    title=f"Recall: multi-strategy ({len(results)} results)",
                    border_style="blue",
                ))
            else:
                console.print("[dim]  No results[/dim]")
            for err in recall_out.strategy_errors:
                console.print(f"[dim]  strategy error: {err.message}[/dim]")
        except Exception as e:
            console.print(f"[red]Error in recall:[/red] {e}")

    elif command == "/brain":
        try:
            count = await hebbs.count()
        except Exception:
            count = "?"
        llm_label = "mock" if mock_llm else f"{cfg.llm.conversation_provider}/{cfg.llm.conversation_model}" if cfg else "?"
        brain_lines = [
            f"[bold]Entity:[/bold]            {entity}",
            f"[bold]Total memories:[/bold]    {count}",
            f"[bold]LLM provider:[/bold]      {llm_label}",
            f"[bold]Server:[/bold]            {cfg.hebbs.address if cfg else '?'}",
        ]
        try:
            ins = await hebbs.insights(entity_id=entity, max_results=5)
            brain_lines.append(f"[bold]Insights:[/bold]          {len(ins)} for this entity")
        except Exception:
            brain_lines.append("[bold]Insights:[/bold]          ?")
        console.print(Panel(
            "\n".join(brain_lines),
            title="HEBBS Engine State",
            border_style="cyan",
        ))

    elif command == "/stats":
        # --- HEBBS Engine Stats ---
        hs = agent.hebbs_stats
        hebbs_table = Table(title="HEBBS Engine Stats", show_header=True, header_style="bold cyan")
        hebbs_table.add_column("Metric", style="white")
        hebbs_table.add_column("Value", justify="right", style="cyan")

        try:
            health = await hebbs.health()
            hebbs_table.add_row("Server version", health.version)
            hebbs_table.add_row("Total memories (server)", f"{health.memory_count:,}")
            hebbs_table.add_row("Uptime", f"{health.uptime_seconds:,}s")
        except Exception:
            hebbs_table.add_row("Server", "[dim]unavailable[/dim]")

        hebbs_table.add_row("Conversation turns", str(hs.turns))
        hebbs_table.add_row("Memories created", str(hs.memories_created))
        hebbs_table.add_row("Memories recalled", str(hs.memories_recalled))
        hebbs_table.add_row("Primed memories", str(hs.primed_memories))
        mm = agent.memory_manager
        hebbs_table.add_row("Recall calls", str(hs.recall_calls))
        hebbs_table.add_row("Remember calls", str(hs.remember_calls))
        if mm.recall_batches > 0:
            hebbs_table.add_row("Avg recall latency", f"{mm.total_recall_ms / mm.recall_batches:,.1f} ms")
            hebbs_table.add_row("Total recall latency", f"{mm.total_recall_ms:,.0f} ms")
        if mm.remember_batches > 0:
            hebbs_table.add_row("Avg remember latency", f"{mm.total_remember_ms / mm.remember_batches:,.1f} ms")
            hebbs_table.add_row("Total remember latency", f"{mm.total_remember_ms:,.0f} ms")
        if mm.prime_calls > 0:
            hebbs_table.add_row("Avg prime latency", f"{mm.total_prime_ms / mm.prime_calls:,.1f} ms")
            hebbs_table.add_row("Total prime latency", f"{mm.total_prime_ms:,.0f} ms")
        if hs.subscribe_pushes:
            hebbs_table.add_row("Subscribe pushes", str(hs.subscribe_pushes))
        if hs.reflect_runs:
            hebbs_table.add_row("Reflect runs", str(hs.reflect_runs))
        if hs.forget_runs:
            hebbs_table.add_row("Forget runs", str(hs.forget_runs))
        console.print(hebbs_table)

        # --- LLM Usage Stats ---
        llm_stats = agent.llm_client.stats
        if llm_stats.total_calls == 0:
            console.print("[dim]No LLM calls yet.[/dim]")
            return
        llm_table = Table(title="LLM Usage Stats", show_header=True, header_style="bold")
        llm_table.add_column("Metric", style="white")
        llm_table.add_column("Value", justify="right", style="cyan")
        llm_table.add_row("Total API calls", str(llm_stats.total_calls))
        llm_table.add_row("Input tokens", f"{llm_stats.total_input_tokens:,}")
        llm_table.add_row("Output tokens", f"{llm_stats.total_output_tokens:,}")
        llm_table.add_row("Total latency", f"{llm_stats.total_latency_ms:,.0f} ms")
        llm_table.add_row("Avg latency/call", f"{llm_stats.total_latency_ms / llm_stats.total_calls:,.0f} ms")
        llm_table.add_row("Est. cost", f"${llm_stats.estimated_cost_usd:.4f}")
        for role, count in llm_stats.calls_by_role.items():
            llm_table.add_row(f"  {role} calls", str(count))
        console.print(llm_table)

    elif command == "/reflect":
        await agent.run_reflect(entity_id=entity)
    elif command == "/forget":
        target = parts[1].strip() if len(parts) > 1 else entity
        await agent.run_forget(entity_id=target)
    elif command == "/insights":
        try:
            ins = await hebbs.insights(entity_id=entity, max_results=10)
            if ins:
                lines = []
                for i, m in enumerate(ins, 1):
                    lines.append(f"  {i}. [{m.importance:.2f}] {m.content}")
                console.print(Panel(
                    "\n".join(lines),
                    title=f'Insights for "{entity}" ({len(ins)} total)',
                    border_style="cyan",
                ))
            else:
                console.print("[dim]No insights yet. Run /reflect to generate them.[/dim]")
        except Exception as e:
            console.print(f"[red]Error:[/red] {e}")
    elif command == "/count":
        try:
            c = await hebbs.count()
            console.print(f"[bold]Total memories across all entities:[/bold] {c}")
        except Exception as e:
            console.print(f"[red]Error:[/red] {e}")
    elif command == "/session":
        arg = parts[1].strip() if len(parts) > 1 else ""
        if arg:
            new_entity = arg
            await agent.end_session()
            entity = new_entity
            await agent.start_session(entity_id=new_entity)
            console.print(f"[dim]Switched to entity: {new_entity}[/dim]")
        else:
            console.print("[dim]Usage: /session <entity_id>[/dim]")
    elif command == "/help":
        console.print(Panel(_HELP_TEXT, title="Commands", border_style="cyan"))
    else:
        console.print(f"[dim]Unknown command: {command}. Type /help for all commands.[/dim]")


@main.command()
@click.option("--config", "config_path", default=None, help="Config name or path to TOML file")
@click.option(
    "--verbosity", type=click.Choice(["quiet", "normal", "verbose"]),
    default="normal", help="Display verbosity level",
)
@click.option("--all", "run_all", is_flag=True, help="Run all scenarios")
@click.option("--run", "scenario_name", default=None, help="Run a specific scenario by name")
@click.option("--mock-llm/--real-llm", default=True, help="Use mock LLM (default: mock)")
def scenarios(config_path: str | None, verbosity: str, run_all: bool, scenario_name: str | None, mock_llm: bool):
    """Run scripted scenario tests."""
    asyncio.run(_scenarios_async(config_path, verbosity, run_all, scenario_name, mock_llm))


async def _scenarios_async(
    config_path: str | None, verbosity: str, run_all: bool, scenario_name: str | None, mock_llm: bool,
):
    from demo.scenarios import ALL_SCENARIOS

    cfg = _resolve_config(config_path)
    v = VERBOSITY_MAP[verbosity]

    if scenario_name:
        names = [scenario_name]
    elif run_all:
        names = list(ALL_SCENARIOS.keys())
    else:
        console.print("[yellow]Specify --all or --run <name>[/yellow]")
        console.print(f"Available: {', '.join(ALL_SCENARIOS.keys())}")
        return

    results = []
    for name in names:
        cls = ALL_SCENARIOS.get(name)
        if cls is None:
            console.print(f"[red]Unknown scenario:[/red] {name}")
            continue

        console.print(f"\n[bold]Running scenario:[/bold] {name}")
        scenario = cls(config=cfg, verbosity=v, use_mock_llm=mock_llm, console=console)
        result = await scenario.run()
        results.append(result)

        status = "[green]PASS[/green]" if result.passed else "[red]FAIL[/red]"
        console.print(f"  {status} ({result.elapsed_ms:.0f}ms, {len(result.assertions)} assertions)")

        if result.error:
            console.print(f"  [red]Error:[/red] {result.error}")

        for a in result.failed_assertions:
            console.print(f"  [red]FAIL:[/red] {a.name}: {a.message}")

    console.print()
    _print_scenario_summary(results, console)
    await _print_engine_metrics(cfg, console, mock_llm)


async def _print_engine_metrics(cfg: DemoConfig, console: Console, mock_llm: bool) -> None:
    """Fetch and display HEBBS engine metrics after a run."""
    try:
        from hebbs import HebbsClient
        client = HebbsClient(cfg.hebbs.address)
        await client.connect()
        health = await client.health()
        await client.close()

        console.print()
        metrics_table = Table(
            title="HEBBS Engine Metrics",
            show_header=True, header_style="bold cyan",
        )
        metrics_table.add_column("Metric", style="white")
        metrics_table.add_column("Value", justify="right", style="cyan")
        metrics_table.add_row("Server version", health.version)
        metrics_table.add_row("Total memories", f"{health.memory_count:,}")
        metrics_table.add_row("Uptime", f"{health.uptime_seconds:,}s")
        metrics_table.add_row("Server address", cfg.hebbs.address)
        llm_label = "mock" if mock_llm else f"{cfg.llm.conversation_provider}/{cfg.llm.conversation_model}"
        metrics_table.add_row("LLM provider", llm_label)
        console.print(metrics_table)
    except Exception:
        pass


def _print_scenario_summary(results: list, console: Console):
    table = Table(title="Scenario Results", show_header=True, header_style="bold")
    table.add_column("Scenario", style="white")
    table.add_column("Status", justify="center")
    table.add_column("Assertions", justify="right")
    table.add_column("Passed", justify="right")
    table.add_column("Failed", justify="right")
    table.add_column("Time", justify="right")

    total_pass = 0
    total_fail = 0
    total_assertions = 0
    total_assertions_passed = 0
    total_time_ms = 0.0

    for r in results:
        passed = sum(1 for a in r.assertions if a.passed)
        failed = sum(1 for a in r.assertions if not a.passed)
        total_pass += (1 if r.passed else 0)
        total_fail += (0 if r.passed else 1)
        total_assertions += len(r.assertions)
        total_assertions_passed += passed
        total_time_ms += r.elapsed_ms
        status = "[green]PASS[/green]" if r.passed else "[red]FAIL[/red]"
        table.add_row(
            r.name, status,
            str(len(r.assertions)), str(passed), str(failed),
            f"{r.elapsed_ms:.0f}ms",
        )

    console.print(table)

    summary_lines = [
        f"[bold]{total_pass}/{total_pass + total_fail} scenarios passed[/bold]",
        f"[dim]{total_assertions_passed}/{total_assertions} assertions passed[/dim]",
        f"[dim]Total wall time: {total_time_ms / 1000:.1f}s[/dim]",
    ]
    if total_pass == total_pass + total_fail:
        summary_lines[0] = f"[bold green]{total_pass}/{total_pass + total_fail} scenarios passed[/bold green]"
    console.print("\n".join(summary_lines))


if __name__ == "__main__":
    main()

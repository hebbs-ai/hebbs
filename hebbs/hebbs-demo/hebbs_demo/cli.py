"""Click CLI: interactive, scenarios, compare-llm, compare-embeddings.

Entry point for the hebbs-demo command.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

import click
from rich.console import Console
from rich.panel import Panel
from rich.table import Table
from rich.text import Text

from hebbs_demo.config import DemoConfig
from hebbs_demo.display import DisplayManager, Verbosity


console = Console()

VERBOSITY_MAP = {
    "quiet": Verbosity.QUIET,
    "normal": Verbosity.NORMAL,
    "verbose": Verbosity.VERBOSE,
}


def _resolve_config(config_path: str | None) -> DemoConfig:
    if config_path:
        return DemoConfig.from_toml(config_path)
    default_path = Path("configs/openai.toml")
    if default_path.exists():
        return DemoConfig.from_toml(default_path)
    return DemoConfig.default()


def _open_hebbs(config: DemoConfig, use_mock_embedder: bool = True):
    from hebbs import HEBBS
    data_dir = config.hebbs.data_dir
    Path(data_dir).mkdir(parents=True, exist_ok=True)
    return HEBBS.open(
        data_dir=data_dir,
        use_mock_embedder=use_mock_embedder,
        embedding_dimensions=384,
    )


@click.group()
@click.version_option(version="0.3.0", prog_name="hebbs-demo")
def main():
    """HEBBS Demo: AI Sales Intelligence Agent."""
    pass


_WELCOME_BANNER = """\
[bold cyan]
   ╔═══════════════════════════════════════════════════════════╗
   ║          HEBBS Demo — Meet "Atlas"                       ║
   ║          Your AI Sales Agent for HEBBS                   ║
   ╚═══════════════════════════════════════════════════════════╝[/bold cyan]

[dim]Atlas sells HEBBS — a cognitive memory engine for AI applications.
You are the prospect. Every message is embedded, recalled, and
remembered — watch the engine work in real time.[/dim]
"""

_GUIDED_TOPICS = """\
[bold]Try these to see different HEBBS recall strategies in action:[/bold]

  [cyan]1.[/cyan] "Tell me about HEBBS"                     [dim]-> similarity recall[/dim]
  [cyan]2.[/cyan] "What have we discussed so far?"           [dim]-> temporal recall[/dim]
  [cyan]3.[/cyan] "What led you to that recommendation?"     [dim]-> causal recall[/dim]
  [cyan]4.[/cyan] "Any companies with similar needs?"        [dim]-> analogical recall[/dim]
  [cyan]5.[/cyan] Type [cyan]/reflect[/cyan] then "What patterns have you learned?"

[dim]Or just chat naturally — all four strategies run on every turn.[/dim]"""


_HELP_TEXT = """\
[bold]Conversation[/bold]
  Just type naturally — you are the sales prospect, Atlas responds.

[bold]Inspect HEBBS Brain[/bold]
  [cyan]/memories[/cyan]          Show all stored memories for this entity
  [cyan]/recall[/cyan] <query>    Manually query HEBBS recall with a cue
  [cyan]/prompt[/cyan]            Show the full system prompt sent to the LLM
  [cyan]/brain[/cyan]             Show engine state: memory count, entity, config
  [cyan]/stats[/cyan]             Show LLM token usage and estimated cost

[bold]Engine Operations[/bold]
  [cyan]/reflect[/cyan]           Trigger HEBBS reflect (generate insights from clusters)
  [cyan]/forget[/cyan] [entity]   GDPR-forget all memories for an entity
  [cyan]/insights[/cyan]          Show accumulated insights for this entity
  [cyan]/count[/cyan]             Total memory count across all entities

[bold]Session[/bold]
  [cyan]/session[/cyan] <entity>  Switch to a different prospect entity
  [cyan]/help[/cyan]              Show this help
  [cyan]quit[/cyan]               Exit"""


@main.command()
@click.option("--config", "config_path", default=None, help="Path to TOML config file")
@click.option(
    "--verbosity", type=click.Choice(["quiet", "normal", "verbose"]),
    default="verbose", help="Display verbosity level",
)
@click.option("--mock-llm", is_flag=True, help="Use mock LLM (no API keys needed)")
@click.option("--onnx", is_flag=True, help="Use real ONNX embedder (downloads BGE-small model on first run)")
@click.option("--entity", default="prospect", help="Entity ID for the conversation")
def interactive(config_path: str | None, verbosity: str, mock_llm: bool, onnx: bool, entity: str):
    """Start an interactive conversation with the AI sales agent."""
    from hebbs_demo.agent import SalesAgent
    from hebbs_demo.llm_client import LlmClient, MockLlmClient

    cfg = _resolve_config(config_path)
    warnings = cfg.validate()
    if warnings and not mock_llm:
        for w in warnings:
            console.print(f"[yellow]Warning:[/yellow] {w}")
        console.print("[dim]Use --mock-llm to run without API keys[/dim]")
        console.print()

    use_mock_embedder = not onnx
    display = DisplayManager(VERBOSITY_MAP[verbosity], console)

    console.print(_WELCOME_BANNER)

    try:
        hebbs = _open_hebbs(cfg, use_mock_embedder=use_mock_embedder)
    except Exception as e:
        console.print(f"[red]Failed to open HEBBS engine:[/red] {e}")
        sys.exit(1)

    try:
        agent = SalesAgent(
            config=cfg,
            hebbs=hebbs,
            display=display,
            use_mock_llm=mock_llm,
        )

        llm_label = "[yellow]mock[/yellow]" if mock_llm else f"[green]{cfg.llm.conversation_provider}/{cfg.llm.conversation_model}[/green]"
        embed_label = "[green]ONNX/BGE-small-en-v1.5[/green]" if onnx else "[yellow]mock (deterministic hash)[/yellow]"
        console.print(f"  LLM:       {llm_label}")
        console.print(f"  Embedder:  {embed_label}")
        console.print(f"  Entity:    [bold]{entity}[/bold]")
        console.print(f"  Verbosity: {verbosity}")
        console.print()

        agent.start_session(entity_id=entity, session_num=1)

        console.print()
        console.print(Panel(_GUIDED_TOPICS, title="Getting Started", border_style="green"))
        console.print()
        console.print("[dim]Type /help for all commands. Atlas will ask for your name, email, and purpose first.[/dim]")
        console.print()

        while True:
            try:
                user_input = console.input("[bold blue]You:[/bold blue] ").strip()
            except (EOFError, KeyboardInterrupt):
                console.print("\n[dim]Goodbye![/dim]")
                break

            if not user_input:
                continue

            if user_input.lower() in ("quit", "exit", "q"):
                console.print("[dim]Goodbye![/dim]")
                break

            if user_input.startswith("/"):
                _handle_command(
                    user_input, agent, hebbs, entity, console,
                    mock_llm=mock_llm, onnx=onnx, cfg=cfg,
                )
                continue

            agent.process_turn(
                prospect_message=user_input,
                recall_strategies=["similarity", "temporal", "causal", "analogical"],
            )

    finally:
        agent.end_session()
        hebbs.close()


def _handle_command(
    cmd: str, agent, hebbs, entity: str, console: Console,
    *, mock_llm: bool = False, onnx: bool = False, cfg: DemoConfig | None = None,
):
    from hebbs_demo.prompts import SYSTEM_SALES_AGENT

    parts = cmd.strip().split(maxsplit=1)
    command = parts[0].lower()

    if command == "/memories":
        try:
            prime_out = hebbs.prime(
                entity_id=entity,
                max_memories=100,
            )
            memories = prime_out.results
            if not memories:
                console.print(Panel(
                    "[dim]No memories stored yet for this entity.[/dim]",
                    title=f"HEBBS Brain — {entity}",
                    border_style="cyan",
                ))
                return
            table = Table(
                title=f"Stored Memories for \"{entity}\" ({len(memories)} total)",
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
                kind = mem.kind.value if hasattr(mem.kind, "value") else str(mem.kind)
                imp = f"{mem.importance:.1f}"
                ctx_parts = []
                if mem.context:
                    for k, v in list(mem.context.items())[:4]:
                        ctx_parts.append(f"{k}={v}")
                ctx_str = ", ".join(ctx_parts) if ctx_parts else "-"
                kind_style = "cyan" if kind.lower() == "insight" else "white"
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
            recall_out = hebbs.recall(
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
                    score = f"{r.score:.3f}" if hasattr(r, "score") else "?"
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

    elif command == "/prompt":
        prompt_text = SYSTEM_SALES_AGENT.strip()
        console.print(Panel(
            prompt_text,
            title="System Prompt — Agent Persona (\"Atlas\")",
            border_style="green",
            padding=(1, 2),
        ))
        if cfg:
            console.print()
            console.print(f"  [dim]Conversation LLM:[/dim]  {cfg.llm.conversation_provider}/{cfg.llm.conversation_model}")
            console.print(f"  [dim]Extraction LLM:[/dim]   {cfg.llm.extraction_provider}/{cfg.llm.extraction_model}")

    elif command == "/brain":
        try:
            count = hebbs.count()
        except Exception:
            count = "?"
        llm_label = "mock" if mock_llm else f"{cfg.llm.conversation_provider}/{cfg.llm.conversation_model}" if cfg else "?"
        embed_label = "ONNX/BGE-small-en-v1.5 (384-dim)" if onnx else "mock (deterministic hash, 384-dim)"
        brain_lines = [
            f"[bold]Entity:[/bold]            {entity}",
            f"[bold]Total memories:[/bold]    {count}",
            f"[bold]LLM provider:[/bold]      {llm_label}",
            f"[bold]Embedder:[/bold]          {embed_label}",
            f"[bold]Data dir:[/bold]          {cfg.hebbs.data_dir if cfg else '?'}",
        ]
        try:
            ins = hebbs.insights(entity_id=entity, max_results=5)
            brain_lines.append(f"[bold]Insights:[/bold]          {len(ins)} for this entity")
        except Exception:
            brain_lines.append("[bold]Insights:[/bold]          ?")
        console.print(Panel(
            "\n".join(brain_lines),
            title="HEBBS Engine State",
            border_style="cyan",
        ))

    elif command == "/stats":
        stats = agent.llm_client.stats
        if stats.total_calls == 0:
            console.print("[dim]No LLM calls yet.[/dim]")
            return
        table = Table(title="LLM Usage Stats", show_header=True, header_style="bold")
        table.add_column("Metric", style="white")
        table.add_column("Value", justify="right", style="cyan")
        table.add_row("Total API calls", str(stats.total_calls))
        table.add_row("Input tokens", f"{stats.total_input_tokens:,}")
        table.add_row("Output tokens", f"{stats.total_output_tokens:,}")
        table.add_row("Total latency", f"{stats.total_latency_ms:,.0f} ms")
        table.add_row("Avg latency/call", f"{stats.total_latency_ms / stats.total_calls:,.0f} ms")
        table.add_row("Est. cost", f"${stats.estimated_cost_usd:.4f}")
        for role, count in stats.calls_by_role.items():
            table.add_row(f"  {role} calls", str(count))
        console.print(table)

    elif command == "/reflect":
        agent.run_reflect(entity_id=entity)
    elif command == "/forget":
        target = parts[1].strip() if len(parts) > 1 else entity
        agent.run_forget(entity_id=target)
    elif command == "/insights":
        try:
            ins = hebbs.insights(entity_id=entity, max_results=10)
            if ins:
                lines = []
                for i, m in enumerate(ins, 1):
                    lines.append(f"  {i}. [{m.importance:.2f}] {m.content}")
                console.print(Panel(
                    "\n".join(lines),
                    title=f"Insights for \"{entity}\" ({len(ins)} total)",
                    border_style="cyan",
                ))
            else:
                console.print("[dim]No insights yet. Run /reflect to generate them.[/dim]")
        except Exception as e:
            console.print(f"[red]Error:[/red] {e}")
    elif command == "/count":
        try:
            c = hebbs.count()
            console.print(f"[bold]Total memories across all entities:[/bold] {c}")
        except Exception as e:
            console.print(f"[red]Error:[/red] {e}")
    elif command == "/session":
        arg = parts[1].strip() if len(parts) > 1 else ""
        if arg:
            new_entity = arg
            agent.end_session()
            entity = new_entity
            agent.start_session(entity_id=new_entity)
            console.print(f"[dim]Switched to entity: {new_entity}[/dim]")
        else:
            console.print("[dim]Usage: /session <entity_id>[/dim]")
    elif command == "/help":
        console.print(Panel(_HELP_TEXT, title="Commands", border_style="cyan"))
    else:
        console.print(f"[dim]Unknown command: {command}. Type /help for all commands.[/dim]")


@main.command()
@click.option("--config", "config_path", default=None, help="Path to TOML config file")
@click.option(
    "--verbosity", type=click.Choice(["quiet", "normal", "verbose"]),
    default="normal", help="Display verbosity level",
)
@click.option("--all", "run_all", is_flag=True, help="Run all scenarios")
@click.option("--run", "scenario_name", default=None, help="Run a specific scenario by name")
@click.option("--mock-llm/--real-llm", default=True, help="Use mock LLM (default: mock)")
@click.option("--onnx", is_flag=True, help="Use real ONNX embedder (downloads BGE-small model on first run)")
def scenarios(config_path: str | None, verbosity: str, run_all: bool, scenario_name: str | None, mock_llm: bool, onnx: bool):
    """Run scripted scenario tests."""
    from hebbs_demo.scenarios import ALL_SCENARIOS

    cfg = _resolve_config(config_path)
    v = VERBOSITY_MAP[verbosity]
    use_mock_embedder = not onnx

    if onnx:
        console.print("[dim]Using ONNX embedder (BGE-small-en-v1.5, 384-dim). First run downloads ~33MB model.[/dim]")

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
        scenario = cls(config=cfg, verbosity=v, use_mock_llm=mock_llm, use_mock_embedder=use_mock_embedder, console=console)
        result = scenario.run()
        results.append(result)

        status = "[green]PASS[/green]" if result.passed else "[red]FAIL[/red]"
        console.print(f"  {status} ({result.elapsed_ms:.0f}ms, {len(result.assertions)} assertions)")

        if result.error:
            console.print(f"  [red]Error:[/red] {result.error}")

        for a in result.failed_assertions:
            console.print(f"  [red]FAIL:[/red] {a.name}: {a.message}")

    console.print()
    _print_scenario_summary(results, console)


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

    for r in results:
        passed = sum(1 for a in r.assertions if a.passed)
        failed = sum(1 for a in r.assertions if not a.passed)
        total_pass += (1 if r.passed else 0)
        total_fail += (0 if r.passed else 1)
        status = "[green]PASS[/green]" if r.passed else "[red]FAIL[/red]"
        table.add_row(
            r.name, status,
            str(len(r.assertions)), str(passed), str(failed),
            f"{r.elapsed_ms:.0f}ms",
        )

    console.print(table)
    console.print(f"\n[bold]{total_pass}/{total_pass + total_fail} scenarios passed[/bold]")


@main.command("compare-llm")
@click.option("--config", "config_path", default=None, help="Path to TOML config file")
@click.option(
    "--verbosity", type=click.Choice(["quiet", "normal", "verbose"]),
    default="quiet", help="Display verbosity level",
)
def compare_llm(config_path: str | None, verbosity: str):
    """Compare LLM providers on the same scenario."""
    from hebbs_demo.reports.comparison import run_llm_comparison

    cfg = _resolve_config(config_path)
    v = VERBOSITY_MAP[verbosity]
    run_llm_comparison(cfg, v, console)


@main.command("compare-embeddings")
@click.option("--configs", default=None, help="Comma-separated config file paths")
@click.option(
    "--verbosity", type=click.Choice(["quiet", "normal", "verbose"]),
    default="quiet", help="Display verbosity level",
)
def compare_embeddings(configs: str | None, verbosity: str):
    """Compare embedding providers on the same scenario."""
    from hebbs_demo.reports.comparison import run_embedding_comparison

    if not configs:
        console.print("[yellow]Usage: --configs configs/onnx-embed.toml,configs/openai-embed.toml[/yellow]")
        return

    config_paths = [p.strip() for p in configs.split(",")]
    v = VERBOSITY_MAP[verbosity]
    run_embedding_comparison(config_paths, v, console)


if __name__ == "__main__":
    main()

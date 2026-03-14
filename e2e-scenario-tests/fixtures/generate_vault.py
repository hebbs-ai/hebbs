#!/usr/bin/env python3
"""Generate test vaults of configurable sizes for HEBBS e2e scenario testing.

Produces a directory of markdown files with realistic content, frontmatter,
wiki-links, tags, and edge cases. Fully deterministic when seeded.

Usage:
    python generate_vault.py --size small --output ./test-vault --seed 42
"""

import argparse
import json
import os
import random
import string
import textwrap
from datetime import datetime, timedelta
from pathlib import Path

# ---------------------------------------------------------------------------
# Size presets
# ---------------------------------------------------------------------------
SIZE_MAP = {
    "small": 50,
    "medium": 500,
    "large": 5000,
    "xl": 10000,
}

# ---------------------------------------------------------------------------
# Word / phrase pools (no lorem ipsum)
# ---------------------------------------------------------------------------
NOUNS = [
    "system", "module", "pipeline", "service", "endpoint", "database",
    "schema", "migration", "deployment", "cluster", "node", "container",
    "workflow", "scheduler", "queue", "cache", "index", "shard", "replica",
    "gateway", "proxy", "firewall", "certificate", "token", "session",
    "payload", "response", "request", "header", "middleware", "controller",
    "handler", "resolver", "adapter", "factory", "observer", "strategy",
    "template", "context", "provider", "consumer", "publisher", "subscriber",
    "metric", "alert", "dashboard", "report", "audit", "log", "trace",
    "budget", "milestone", "sprint", "epic", "story", "ticket", "backlog",
    "stakeholder", "customer", "team", "engineer", "manager", "analyst",
    "roadmap", "timeline", "objective", "outcome", "initiative", "proposal",
]

VERBS = [
    "deploy", "migrate", "refactor", "optimize", "scale", "monitor",
    "configure", "integrate", "validate", "test", "review", "merge",
    "document", "investigate", "resolve", "implement", "design", "plan",
    "evaluate", "prioritize", "schedule", "coordinate", "delegate",
    "approve", "release", "rollback", "provision", "decommission",
    "benchmark", "profile", "debug", "trace", "audit", "secure",
]

ADJECTIVES = [
    "scalable", "reliable", "distributed", "async", "concurrent",
    "ephemeral", "persistent", "stateless", "idempotent", "resilient",
    "incremental", "automated", "manual", "critical", "optional",
    "primary", "secondary", "upstream", "downstream", "internal",
    "external", "legacy", "modern", "experimental", "stable",
    "high-priority", "low-latency", "fault-tolerant", "cloud-native",
]

TECH_TERMS = [
    "Kubernetes", "Docker", "PostgreSQL", "Redis", "Kafka", "gRPC",
    "REST API", "GraphQL", "CI/CD", "Terraform", "Ansible", "Prometheus",
    "Grafana", "Elasticsearch", "S3", "Lambda", "CloudFront", "Nginx",
    "HAProxy", "RabbitMQ", "MongoDB", "DynamoDB", "Snowflake", "dbt",
    "Airflow", "Spark", "Flink", "Kinesis", "EventBridge", "Step Functions",
    "React", "Next.js", "TypeScript", "Python", "Go", "Rust",
]

PEOPLE = [
    "Alice", "Bob", "Carol", "Dave", "Eve", "Frank", "Grace", "Heidi",
    "Ivan", "Judy", "Karl", "Liam", "Mona", "Nora", "Oscar", "Priya",
    "Quinn", "Rita", "Sam", "Tina", "Uma", "Vince", "Wendy", "Xander",
]

TOPICS = [
    "architecture", "performance", "security", "reliability", "observability",
    "testing", "onboarding", "documentation", "cost-optimization", "migration",
    "incident-response", "capacity-planning", "feature-flags", "data-pipeline",
    "auth", "billing", "search", "notifications", "analytics", "compliance",
]

AUTHORS = [
    "jsmith", "akumar", "mchen", "ljohnson", "pgarcia", "twilliams",
    "rbrown", "sdavis", "kmiller", "nwilson", "jmoore", "ataylor",
]

# ---------------------------------------------------------------------------
# Filename generation
# ---------------------------------------------------------------------------
FILENAME_PREFIXES = [
    "meeting-notes", "api-design", "project-update", "sprint-review",
    "architecture-decision", "runbook", "postmortem", "onboarding-guide",
    "release-notes", "tech-spec", "design-doc", "retrospective",
    "brainstorm", "investigation", "proposal", "research-notes",
    "weekly-sync", "quarterly-review", "incident-report", "changelog",
    "feature-spec", "migration-plan", "performance-review", "security-audit",
    "capacity-plan", "cost-analysis", "user-research", "competitive-analysis",
    "roadmap-update", "team-standup", "one-on-one", "all-hands",
    "data-model", "schema-design", "test-plan", "deployment-guide",
    "troubleshooting", "faq", "glossary", "style-guide",
]

FILENAME_SUFFIXES = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep",
    "oct", "nov", "dec", "q1", "q2", "q3", "q4", "v1", "v2", "v3",
    "draft", "final", "wip", "2024", "2025", "2026",
]

SUBDIRS = [
    "", "", "", "",  # weight toward root
    "projects", "meetings", "specs", "guides", "incidents",
    "research", "planning", "teams", "archive",
]


def generate_filename(rng, used_names):
    """Generate a unique kebab-case markdown filename."""
    for _ in range(200):
        prefix = rng.choice(FILENAME_PREFIXES)
        suffix = rng.choice(FILENAME_SUFFIXES)
        if rng.random() < 0.3:
            mid = rng.choice(TOPICS)
            name = f"{prefix}-{mid}-{suffix}"
        else:
            name = f"{prefix}-{suffix}"
        subdir = rng.choice(SUBDIRS)
        rel = os.path.join(subdir, f"{name}.md") if subdir else f"{name}.md"
        if rel not in used_names:
            used_names.add(rel)
            return rel
    # fallback: add random digits
    rel = f"{prefix}-{suffix}-{rng.randint(100,999)}.md"
    used_names.add(rel)
    return rel


# ---------------------------------------------------------------------------
# Content generation helpers
# ---------------------------------------------------------------------------

SENTENCE_TEMPLATES = [
    "The {adj} {noun} needs to be {verb}d before the next release.",
    "We discussed the {noun} {noun2} integration during the meeting with {person}.",
    "{person} will {verb} the {adj} {noun} by end of week.",
    "After the incident, the team decided to {verb} the {noun} and improve {noun2} handling.",
    "The {tech} {noun} has been running in production for three months without issues.",
    "We should {verb} the {noun} to reduce latency and improve throughput.",
    "According to {person}, the {adj} {noun} approach will save us significant time.",
    "The {noun} was originally designed as a {adj} {noun2}, but scope has expanded.",
    "Next steps: {verb} the {noun}, coordinate with {person}, and update the {noun2}.",
    "Performance testing showed the {tech} {noun} handles 10x the expected load.",
    "The new {adj} {noun} replaces the legacy {noun2} that was deprecated last quarter.",
    "Key takeaway: always {verb} the {noun} before pushing changes to production.",
    "{person} raised concerns about the {adj} {noun} during the review.",
    "We need better {noun} coverage, especially around the {tech} integration layer.",
    "The {noun} and {noun2} are tightly coupled, making it hard to {verb} independently.",
    "Action item: {person} to draft a proposal for the {adj} {noun} by Friday.",
    "The {tech} upgrade improved {noun} performance by approximately 40 percent.",
    "Current {noun} utilization is at 78 percent, approaching the threshold for scaling.",
    "The {adj} {noun2} feature was shipped behind a feature flag last sprint.",
    "Team consensus was to {verb} the existing {noun} rather than build from scratch.",
]

HEADING_TEMPLATES = [
    "Overview", "Background", "Context", "Summary", "Action Items",
    "Discussion", "Decisions", "Next Steps", "Open Questions",
    "Architecture", "Implementation Details", "Testing Strategy",
    "Performance Considerations", "Security Review", "Dependencies",
    "Timeline", "Risks and Mitigations", "Metrics", "Rollout Plan",
    "Retrospective Notes", "Lessons Learned", "Follow-ups",
    "Technical Details", "Requirements", "Acceptance Criteria",
    "Design Alternatives", "Cost Analysis", "Resource Allocation",
    "Stakeholder Feedback", "User Impact", "Migration Steps",
]


def make_sentence(rng):
    """Generate a single realistic sentence."""
    template = rng.choice(SENTENCE_TEMPLATES)
    return template.format(
        adj=rng.choice(ADJECTIVES),
        noun=rng.choice(NOUNS),
        noun2=rng.choice(NOUNS),
        verb=rng.choice(VERBS),
        person=rng.choice(PEOPLE),
        tech=rng.choice(TECH_TERMS),
    )


def make_paragraph(rng, min_sentences=2, max_sentences=6):
    """Generate a paragraph of realistic text."""
    count = rng.randint(min_sentences, max_sentences)
    return " ".join(make_sentence(rng) for _ in range(count))


def make_bullet_list(rng, min_items=2, max_items=6):
    """Generate a markdown bullet list."""
    count = rng.randint(min_items, max_items)
    items = []
    for _ in range(count):
        items.append(f"- {make_sentence(rng)}")
    return "\n".join(items)


# ---------------------------------------------------------------------------
# Frontmatter generation
# ---------------------------------------------------------------------------

def make_frontmatter(rng):
    """Generate YAML frontmatter."""
    lines = ["---"]
    lines.append(f"title: \"{rng.choice(FILENAME_PREFIXES).replace('-', ' ').title()}\"")
    if rng.random() < 0.7:
        lines.append(f"author: {rng.choice(AUTHORS)}")
    if rng.random() < 0.8:
        base = datetime(2024, 1, 1)
        delta = timedelta(days=rng.randint(0, 800))
        lines.append(f"date: {(base + delta).strftime('%Y-%m-%d')}")
    if rng.random() < 0.6:
        tag_count = rng.randint(1, 4)
        tags = rng.sample(TOPICS, min(tag_count, len(TOPICS)))
        lines.append(f"tags: [{', '.join(tags)}]")
    if rng.random() < 0.3:
        lines.append(f"importance: {rng.choice(['low', 'medium', 'high', 'critical'])}")
    lines.append("---")
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# Wiki-link and tag injection
# ---------------------------------------------------------------------------

def inject_wiki_links(rng, content, all_filenames, count=None):
    """Insert wiki-links to other vault files into the content."""
    if count is None:
        count = rng.randint(1, 5)
    # Pick random target files (strip .md and path for wiki-link)
    targets = rng.sample(all_filenames, min(count, len(all_filenames)))
    links = []
    for t in targets:
        name = Path(t).stem
        links.append(f"[[{name}]]")
    link_line = "Related: " + ", ".join(links)
    # Insert after the first paragraph break
    parts = content.split("\n\n", 1)
    if len(parts) == 2:
        return parts[0] + "\n\n" + link_line + "\n\n" + parts[1]
    return content + "\n\n" + link_line


def inject_tags(rng, content, count=None):
    """Insert inline tags into the content."""
    if count is None:
        count = rng.randint(1, 4)
    tags = []
    for _ in range(count):
        topic = rng.choice(TOPICS)
        if rng.random() < 0.3:
            subtopic = rng.choice(NOUNS)
            tags.append(f"#{topic}/{subtopic}")
        else:
            tags.append(f"#{topic}")
    tag_line = " ".join(tags)
    return content + "\n\n" + tag_line + "\n"


# ---------------------------------------------------------------------------
# File content generation
# ---------------------------------------------------------------------------

def generate_normal_file(rng, has_frontmatter, has_links, has_tags, all_filenames):
    """Generate a normal markdown file with headings and paragraphs."""
    parts = []

    if has_frontmatter:
        parts.append(make_frontmatter(rng))
        parts.append("")

    # Weighted heading count: favor 3-5
    weights = [1, 2, 5, 8, 8, 5, 3, 1]  # for 1..8
    heading_count = rng.choices(range(1, 9), weights=weights, k=1)[0]

    used_headings = set()
    for i in range(heading_count):
        # Pick a unique heading
        for _ in range(50):
            h = rng.choice(HEADING_TEMPLATES)
            if h not in used_headings:
                used_headings.add(h)
                break

        level = 2 if i > 0 else 1
        if i > 0 and rng.random() < 0.3:
            level = 3
        parts.append(f"{'#' * level} {h}")
        parts.append("")

        para_count = rng.randint(1, 5)
        for _ in range(para_count):
            if rng.random() < 0.2:
                parts.append(make_bullet_list(rng))
            else:
                parts.append(make_paragraph(rng))
            parts.append("")

    content = "\n".join(parts)

    if has_links:
        content = inject_wiki_links(rng, content, all_filenames)

    if has_tags:
        content = inject_tags(rng, content)

    return content


def generate_edge_case(rng, kind):
    """Generate an edge-case file."""
    if kind == "empty":
        return ""
    elif kind == "no-headings":
        return make_paragraph(rng, 3, 8) + "\n\n" + make_paragraph(rng, 2, 5) + "\n"
    elif kind == "frontmatter-only":
        return make_frontmatter(rng) + "\n"
    elif kind == "large":
        # ~50KB of content
        parts = []
        parts.append("# Large Document\n")
        while len("\n".join(parts)) < 50000:
            parts.append(f"## {rng.choice(HEADING_TEMPLATES)} {rng.randint(1, 999)}\n")
            for _ in range(rng.randint(3, 8)):
                parts.append(make_paragraph(rng, 4, 8))
                parts.append("")
        return "\n".join(parts)
    return ""


# ---------------------------------------------------------------------------
# Main generation logic
# ---------------------------------------------------------------------------

def generate_vault(size_name, output_dir, seed):
    """Generate the full test vault."""
    file_count = SIZE_MAP[size_name]
    rng = random.Random(seed)

    output_path = Path(output_dir)
    output_path.mkdir(parents=True, exist_ok=True)

    # Pre-generate all filenames
    used_names = set()
    filenames = []
    for _ in range(file_count):
        filenames.append(generate_filename(rng, used_names))

    # Decide file roles
    indices = list(range(file_count))
    rng.shuffle(indices)

    edge_case_count = max(1, int(file_count * 0.05))
    frontmatter_count = max(1, int(file_count * 0.30))
    link_count = max(1, int(file_count * 0.20))
    tag_count = max(1, int(file_count * 0.10))

    edge_case_indices = set(indices[:edge_case_count])
    remaining = indices[edge_case_count:]

    # From the remaining normal files, assign frontmatter / links / tags
    # These can overlap
    rng.shuffle(remaining)
    frontmatter_indices = set(remaining[:frontmatter_count])
    rng.shuffle(remaining)
    link_indices = set(remaining[:link_count])
    rng.shuffle(remaining)
    tag_indices = set(remaining[:tag_count])

    edge_case_kinds = ["empty", "no-headings", "frontmatter-only", "large"]
    total_sections = 0
    files_with_frontmatter = 0
    files_with_links = 0
    files_with_tags = 0

    for i, fname in enumerate(filenames):
        fpath = output_path / fname
        fpath.parent.mkdir(parents=True, exist_ok=True)

        if i in edge_case_indices:
            kind = edge_case_kinds[i % len(edge_case_kinds)]
            content = generate_edge_case(rng, kind)
            # Count sections in edge cases too
            total_sections += content.count("\n# ") + content.count("\n## ") + content.count("\n### ")
            if content.startswith("# "):
                total_sections += 1
        else:
            has_fm = i in frontmatter_indices
            has_lk = i in link_indices
            has_tg = i in tag_indices
            content = generate_normal_file(rng, has_fm, has_lk, has_tg, filenames)

            # Count sections
            section_count = 0
            for line in content.split("\n"):
                if line.startswith("#"):
                    section_count += 1
            total_sections += section_count

            if has_fm:
                files_with_frontmatter += 1
            if has_lk:
                files_with_links += 1
            if has_tg:
                files_with_tags += 1

        fpath.write_text(content, encoding="utf-8")

    # Write metadata
    metadata = {
        "file_count": file_count,
        "total_sections": total_sections,
        "files_with_frontmatter": files_with_frontmatter,
        "files_with_links": files_with_links,
        "files_with_tags": files_with_tags,
        "edge_case_count": edge_case_count,
        "seed": seed,
    }
    meta_path = output_path / "metadata.json"
    meta_path.write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")

    return metadata


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(
        description="Generate test vaults for HEBBS e2e scenario testing."
    )
    parser.add_argument(
        "--size",
        choices=list(SIZE_MAP.keys()),
        required=True,
        help="Vault size preset: small (50), medium (500), large (5000), xl (10000)",
    )
    parser.add_argument(
        "--output",
        required=True,
        help="Output directory for the generated vault",
    )
    parser.add_argument(
        "--seed",
        type=int,
        default=42,
        help="Random seed for reproducibility (default: 42)",
    )
    args = parser.parse_args()

    print(f"Generating {args.size} vault ({SIZE_MAP[args.size]} files) with seed {args.seed}...")
    metadata = generate_vault(args.size, args.output, args.seed)
    print(f"Done! Vault written to: {args.output}")
    print(f"  Files:          {metadata['file_count']}")
    print(f"  Total sections: {metadata['total_sections']}")
    print(f"  Frontmatter:    {metadata['files_with_frontmatter']}")
    print(f"  Wiki-links:     {metadata['files_with_links']}")
    print(f"  Tags:           {metadata['files_with_tags']}")
    print(f"  Edge cases:     {metadata['edge_case_count']}")
    print(f"  Metadata:       {os.path.join(args.output, 'metadata.json')}")


if __name__ == "__main__":
    main()

import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

export default defineConfig({
  site: "https://docs.hebbs.ai",
  integrations: [
    starlight({
      title: "HEBBS",
      logo: {
        dark: "./src/assets/logo-dark.svg",
        light: "./src/assets/logo-light.svg",
        replacesTitle: true,
      },
      favicon: "/favicon.svg",
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/hebbs-ai/hebbs",
        },
      ],
      customCss: ["./src/styles/custom.css"],
      head: [
        {
          tag: "script",
          content: `document.documentElement.dataset.theme = 'dark';
window.StarlightThemeProvider = { updatePickers() {} };
localStorage.setItem('starlight-theme', 'dark');`,
        },
        {
          tag: "link",
          attrs: {
            rel: "preconnect",
            href: "https://fonts.googleapis.com",
          },
        },
        {
          tag: "link",
          attrs: {
            rel: "preconnect",
            href: "https://fonts.gstatic.com",
            crossorigin: true,
          },
        },
        {
          tag: "link",
          attrs: {
            rel: "stylesheet",
            href: "https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500;700&family=Space+Grotesk:wght@700&display=swap",
          },
        },
      ],
      sidebar: [
        {
          label: "Getting Started",
          items: [
            { label: "Introduction", slug: "getting-started/introduction" },
            { label: "Quickstart", slug: "getting-started/quickstart" },
            { label: "Installation", slug: "getting-started/installation" },
            { label: "Key Concepts", slug: "getting-started/key-concepts" },
          ],
        },
        {
          label: "Files & Sync",
          items: [
            {
              label: "Uploading Files",
              slug: "files/uploading",
            },
            {
              label: "Tuning & Evals",
              slug: "files/tuning",
            },
          ],
        },
        {
          label: "Server",
          items: [
            { label: "Running the Server", slug: "server/running" },
            { label: "Configuration", slug: "server/configuration" },
            { label: "Health & Metrics", slug: "server/health-metrics" },
          ],
        },
        {
          label: "Core Concepts",
          items: [
            { label: "Memory Model", slug: "concepts/memory-model" },
            {
              label: "Recall Strategies",
              slug: "concepts/recall-strategies",
            },
            {
              label: "Importance & Decay",
              slug: "concepts/importance-decay",
            },
            {
              label: "Reflection & Insights",
              slug: "concepts/reflection-insights",
            },
            {
              label: "Entity Isolation",
              slug: "concepts/entity-isolation",
            },
            { label: "Lineage & Edges", slug: "concepts/lineage-edges" },
            {
              label: "Subscribe (Real-time)",
              slug: "concepts/subscribe-realtime",
            },
          ],
        },
        {
          label: "API Reference",
          items: [
            { label: "Overview", slug: "api/overview" },
            { label: "remember", slug: "api/remember" },
            { label: "recall", slug: "api/recall" },
            { label: "revise", slug: "api/revise" },
            { label: "forget", slug: "api/forget" },
            { label: "prime", slug: "api/prime" },
            { label: "subscribe", slug: "api/subscribe" },
            { label: "reflect_policy", slug: "api/reflect-policy" },
            { label: "reflect", slug: "api/reflect" },
            { label: "insights", slug: "api/insights" },
            { label: "Protobuf Schema", slug: "api/protobuf-schema" },
            { label: "REST Endpoints", slug: "api/rest-endpoints" },
            { label: "Error Codes", slug: "api/error-codes" },
          ],
        },
        {
          label: "Python SDK",
          items: [
            { label: "Overview", slug: "python/overview" },
            { label: "Installation", slug: "python/installation" },
            { label: "Quick Start", slug: "python/quickstart" },
            { label: "Client Reference", slug: "python/client-reference" },
            { label: "Types Reference", slug: "python/types-reference" },
            { label: "Error Handling", slug: "python/error-handling" },
            {
              label: "Subscribe Streaming",
              slug: "python/subscribe-streaming",
            },
          ],
        },
        {
          label: "TypeScript SDK",
          items: [
            { label: "Overview", slug: "typescript/overview" },
            { label: "Installation", slug: "typescript/installation" },
            { label: "Quick Start", slug: "typescript/quickstart" },
            {
              label: "Client Reference",
              slug: "typescript/client-reference",
            },
            {
              label: "Types Reference",
              slug: "typescript/types-reference",
            },
            { label: "Error Handling", slug: "typescript/error-handling" },
            {
              label: "Subscribe Streaming",
              slug: "typescript/subscribe-streaming",
            },
          ],
        },
        {
          label: "CLI",
          items: [
            { label: "Overview", slug: "cli/overview" },
            { label: "Installation", slug: "cli/installation" },
            { label: "Commands", slug: "cli/commands" },
            { label: "REPL", slug: "cli/repl" },
            { label: "Output Formats", slug: "cli/output-formats" },
          ],
        },
        {
          label: "Rust SDK",
          items: [
            { label: "Overview", slug: "rust-sdk/overview" },
            { label: "Quick Start", slug: "rust-sdk/quickstart" },
            { label: "Client Reference", slug: "rust-sdk/client-reference" },
            { label: "FFI (C/C++)", slug: "rust-sdk/ffi" },
          ],
        },
        {
          label: "Deployment",
          items: [
            { label: "Overview", slug: "deployment/overview" },
            { label: "Docker", slug: "deployment/docker" },
            { label: "Kubernetes (Helm)", slug: "deployment/kubernetes" },
            { label: "Terraform (AWS)", slug: "deployment/terraform-aws" },
            { label: "Monitoring", slug: "deployment/monitoring" },
            {
              label: "Production Checklist",
              slug: "deployment/production-checklist",
            },
          ],
        },
        {
          label: "Architecture",
          items: [
            { label: "Overview", slug: "architecture/overview" },
            { label: "Scalability", slug: "architecture/scalability" },
          ],
        },
        {
          label: "Cookbooks",
          items: [
            { label: "Overview", slug: "cookbooks" },
            {
              label: "Your First Memory Agent",
              slug: "cookbooks/first-memory-agent",
            },
            {
              label: "Multi-Strategy Recall",
              slug: "cookbooks/multi-strategy-recall",
            },
            {
              label: "Entity-Scoped Memory",
              slug: "cookbooks/entity-scoped-memory",
            },
            {
              label: "Voice Sales Agent",
              slug: "cookbooks/voice-sales-agent",
            },
            {
              label: "Customer Support Agent",
              slug: "cookbooks/customer-support",
            },
            {
              label: "GDPR Compliance",
              slug: "cookbooks/gdpr-compliance",
            },
            {
              label: "Real-time Subscribe",
              slug: "cookbooks/realtime-subscribe",
            },
            {
              label: "Background Learning",
              slug: "cookbooks/background-learning",
            },
            {
              label: "Research Assistant",
              slug: "cookbooks/research-assistant",
            },
            { label: "Causal Chains", slug: "cookbooks/causal-chains" },
            {
              label: "Custom Dashboard",
              slug: "cookbooks/custom-dashboard",
            },
            {
              label: "Monitoring Stack",
              slug: "cookbooks/monitoring-stack",
            },
          ],
        },
        {
          label: "Benchmarks",
          items: [
            { label: "Performance Targets", slug: "benchmarks/targets" },
            {
              label: "Scalability Curves",
              slug: "benchmarks/scalability",
            },
            {
              label: "Cognitive Benchmarks",
              slug: "benchmarks/cognitive",
            },
          ],
        },
      ],
    }),
  ],
});

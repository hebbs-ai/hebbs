/**
 * HEBBS REST Client for enterprise servers.
 *
 * Usage:
 *   const hb = new HebbsRestClient("http://server:8080", { apiKey: "hb_live_sk_..." });
 *   const results = await hb.recall("your query");
 *   console.log(results.text);
 *   await hb.close();
 */

// ── Types ──────────────────────────────────────────────────────────────

export interface RestClientOptions {
  apiKey?: string;
  timeout?: number;
}

export interface RestMemory {
  memoryId: string;
  content: string;
  importance: number;
  decayScore: number;
  entityId?: string;
  filePath?: string;
  kind?: string;
  score: number;
  accessCount: number;
  createdAtUs: number;
  lastAccessedAtUs: number;
  context?: Record<string, unknown>;
}

export interface RestRecallOutput {
  results: RestMemory[];
  count: number;
  indexingPct?: number;
  /** Concatenated content of all results, ready for LLM context. */
  text: string;
}

export interface RestPrimeOutput {
  results: RestMemory[];
  count: number;
  text: string;
}

export interface RestForgetResult {
  forgottenCount: number;
  cascadeCount: number;
}

export interface RestInsightsOutput {
  insights: Record<string, unknown>[];
  count: number;
  text: string;
}

export interface RestStatusOutput {
  status: string;
  version: string;
  engine: string;
}

export interface RestUploadResult {
  uploaded: number;
  files: string[];
}

// ── Client ─────────────────────────────────────────────────────────────

export class HebbsRestClient {
  private endpoint: string;
  private apiKey: string;
  private timeout: number;

  constructor(endpoint: string, options: RestClientOptions = {}) {
    this.endpoint = endpoint.replace(/\/$/, '');
    this.apiKey = options.apiKey ?? process.env.HEBBS_API_KEY ?? '';
    this.timeout = options.timeout ?? 30000;
  }

  async close(): Promise<void> {
    // No persistent connections to clean up with fetch
  }

  // ── Memory Operations ──────────────────────────────────────────────

  async remember(
    content: string,
    options: { importance?: number; entityId?: string; context?: Record<string, unknown> } = {},
  ): Promise<RestMemory> {
    const body: Record<string, unknown> = { content };
    if (options.importance !== undefined) body.importance = options.importance;
    if (options.entityId) body.entity_id = options.entityId;
    if (options.context) body.context = options.context;

    const data = await this.post('/v1/memories', body);
    return toRestMemory(data);
  }

  async recall(
    cue: string,
    options: { topK?: number; entityId?: string; strategy?: string } = {},
  ): Promise<RestRecallOutput> {
    const body: Record<string, unknown> = { cue, top_k: options.topK ?? 10 };
    if (options.entityId) body.entity_id = options.entityId;
    if (options.strategy) body.strategy = options.strategy;

    const data = await this.post('/v1/recall', body);
    const results = (data.results ?? []).map(toRestMemory);
    return {
      results,
      count: data.count ?? results.length,
      indexingPct: data.indexing_pct,
      text: results.map((r: RestMemory) => r.content).join('\n\n'),
    };
  }

  async prime(
    entityId: string,
    options: { maxMemories?: number; similarityCue?: string } = {},
  ): Promise<RestPrimeOutput> {
    const body: Record<string, unknown> = { entity_id: entityId };
    if (options.maxMemories) body.max_memories = options.maxMemories;
    if (options.similarityCue) body.similarity_cue = options.similarityCue;

    const data = await this.post('/v1/prime', body);
    const results = (data.results ?? []).map(toRestMemory);
    return {
      results,
      count: data.count ?? results.length,
      text: results.map((r: RestMemory) => r.content).join('\n\n'),
    };
  }

  async forget(options: { entityId?: string; ids?: string[] } = {}): Promise<RestForgetResult> {
    const body: Record<string, unknown> = {};
    if (options.entityId) body.entity_id = options.entityId;
    if (options.ids) body.ids = options.ids;

    const data = await this.post('/v1/forget', body);
    return {
      forgottenCount: data.forgotten_count ?? 0,
      cascadeCount: data.cascade_count ?? 0,
    };
  }

  async insights(entityId?: string): Promise<RestInsightsOutput> {
    const path = entityId ? `/v1/insights?entity_id=${entityId}` : '/v1/insights';
    const data = await this.get(path);
    const insights = data.insights ?? [];
    return {
      insights,
      count: data.count ?? insights.length,
      text: insights.map((i: Record<string, unknown>) => i.content ?? String(i)).join('\n\n'),
    };
  }

  async status(): Promise<RestStatusOutput> {
    const data = await this.get('/v1/system/health');
    return {
      status: data.status ?? '',
      version: data.version ?? '',
      engine: data.engine ?? '',
    };
  }

  async index(files: { name: string; content: Uint8Array }[]): Promise<RestUploadResult> {
    const formData = new FormData();
    for (const f of files) {
      formData.append('files', new Blob([f.content as unknown as ArrayBuffer]), f.name);
    }

    const resp = await fetch(`${this.endpoint}/v1/upload`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${this.apiKey}` },
      body: formData,
      signal: AbortSignal.timeout(this.timeout),
    });

    const data = (await resp.json()) as Record<string, any>;
    if (!resp.ok) throw new HebbsRestError((data as any).error ?? `HTTP ${resp.status}`, resp.status);

    return {
      uploaded: data.uploaded ?? 0,
      files: data.files ?? [],
    };
  }

  // ── HTTP helpers ───────────────────────────────────────────────────

  private async get(path: string): Promise<Record<string, any>> {
    const resp = await fetch(`${this.endpoint}${path}`, {
      headers: this.headers(),
      signal: AbortSignal.timeout(this.timeout),
    });

    const data = (await resp.json()) as Record<string, any>;
    if (!resp.ok) throw new HebbsRestError((data as any).error ?? `HTTP ${resp.status}`, resp.status);
    return data;
  }

  private async post(path: string, body: Record<string, unknown>): Promise<Record<string, any>> {
    const resp = await fetch(`${this.endpoint}${path}`, {
      method: 'POST',
      headers: { ...this.headers(), 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(this.timeout),
    });

    const data = (await resp.json()) as Record<string, any>;
    if (!resp.ok) throw new HebbsRestError((data as any).error ?? `HTTP ${resp.status}`, resp.status);
    return data;
  }

  private headers(): Record<string, string> {
    const h: Record<string, string> = {};
    if (this.apiKey) h['Authorization'] = `Bearer ${this.apiKey}`;
    return h;
  }
}

// ── Error ──────────────────────────────────────────────────────────────

export class HebbsRestError extends Error {
  readonly statusCode: number;

  constructor(message: string, statusCode: number) {
    super(message);
    this.name = 'HebbsRestError';
    this.statusCode = statusCode;
  }
}

// ── Helpers ────────────────────────────────────────────────────────────

function toRestMemory(d: Record<string, any>): RestMemory {
  const ctx = d.context ?? {};
  return {
    memoryId: d.memory_id ?? '',
    content: d.content ?? '',
    importance: d.importance ?? 0,
    decayScore: d.decay_score ?? 0,
    entityId: d.entity_id,
    filePath: d.file_path ?? ctx.file_path,
    kind: d.kind ?? ctx.layer,
    score: d.score ?? 0,
    accessCount: d.access_count ?? 0,
    createdAtUs: d.created_at_us ?? 0,
    lastAccessedAtUs: d.last_accessed_at_us ?? 0,
    context: d.context,
  };
}

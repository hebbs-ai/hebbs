/**
 * HebbsClient: async gRPC client for the HEBBS cognitive memory engine.
 *
 * ```ts
 * const client = new HebbsClient('localhost:6380', { apiKey: 'hb_...' });
 * await client.connect();
 *
 * const mem = await client.remember({ content: 'Acme Corp uses Salesforce' });
 * const { results } = await client.recall({ cue: 'What CRM does Acme use?' });
 *
 * await client.close();
 * ```
 */

import * as grpc from '@grpc/grpc-js';
import { HebbsConnectionError } from './errors.js';
import {
  MemoryServiceClient,
  SubscribeServiceClient,
  ReflectServiceClient,
  HealthServiceClient,
} from './proto.js';
import { HealthService } from './services/health.js';
import { MemoryService } from './services/memory.js';
import { ReflectService } from './services/reflect.js';
import { SubscribeService, Subscription } from './services/subscribe.js';
import type {
  Edge,
  ForgetResult,
  HealthStatus,
  Memory,
  PrimeOutput,
  RecallOutput,
  RecallStrategyConfig,
  ReflectResult,
  ScoringWeights,
  RememberParams,
  RecallParams,
  PrimeParams,
  ReviseParams,
  ForgetParams,
  SetPolicyParams,
  SubscribeParams,
  ReflectParams,
  InsightsParams,
  ClusterPrompt,
  ProducedInsightInput,
  ReflectCommitResult,
  ReflectPrepareResult,
  ReflectPrepareParams,
  ReflectCommitParams,
  PendingContradiction,
  ContradictionVerdictInput,
  ContradictionCommitResult,
} from './types.js';

export interface HebbsClientOptions {
  apiKey?: string;
  tenantId?: string;
  channelOptions?: Record<string, unknown>;
}

export class HebbsClient {
  private readonly address: string;
  private readonly apiKey?: string;
  private readonly tenantId?: string;
  private readonly channelOptions: Record<string, unknown>;
  private metadata: grpc.Metadata;
  private memoryService: MemoryService | null = null;
  private subscribeService: SubscribeService | null = null;
  private reflectService: ReflectService | null = null;
  private healthService: HealthService | null = null;
  private stubs: grpc.Client[] = [];

  constructor(address: string = 'localhost:6380', options?: HebbsClientOptions) {
    this.address = address;
    this.apiKey =
      options?.apiKey ?? process.env['HEBBS_API_KEY'] ?? undefined;
    this.tenantId = options?.tenantId;
    this.channelOptions = options?.channelOptions ?? {};
    this.metadata = new grpc.Metadata();
    if (this.apiKey) {
      this.metadata.add('authorization', `Bearer ${this.apiKey}`);
    }
  }

  private ensureConnected(): void {
    if (!this.memoryService) {
      throw new HebbsConnectionError(
        "Not connected. Call connect() first, or use 'await using' syntax.",
      );
    }
  }

  async connect(): Promise<HebbsClient> {
    const credentials = grpc.credentials.createInsecure();

    const memStub = new MemoryServiceClient(
      this.address,
      credentials,
      this.channelOptions,
    );
    const subStub = new SubscribeServiceClient(
      this.address,
      credentials,
      this.channelOptions,
    );
    const refStub = new ReflectServiceClient(
      this.address,
      credentials,
      this.channelOptions,
    );
    const hltStub = new HealthServiceClient(
      this.address,
      credentials,
      this.channelOptions,
    );

    this.stubs = [memStub, subStub, refStub, hltStub];

    this.memoryService = new MemoryService(
      memStub,
      this.metadata,
      this.tenantId,
    );
    this.subscribeService = new SubscribeService(
      subStub,
      this.metadata,
      this.tenantId,
    );
    this.reflectService = new ReflectService(
      refStub,
      this.metadata,
      this.tenantId,
    );
    this.healthService = new HealthService(hltStub, this.metadata);

    return this;
  }

  async close(): Promise<void> {
    for (const stub of this.stubs) {
      stub.close();
    }
    this.stubs = [];
    this.memoryService = null;
    this.subscribeService = null;
    this.reflectService = null;
    this.healthService = null;
  }

  // ── MemoryService ──────────────────────────────────────────────────

  async remember(params: RememberParams): Promise<Memory>;
  async remember(
    content: string,
    importance?: number,
    context?: Record<string, unknown>,
    entityId?: string,
    edges?: Edge[],
  ): Promise<Memory>;
  async remember(
    contentOrParams: string | RememberParams,
    importance?: number,
    context?: Record<string, unknown>,
    entityId?: string,
    edges?: Edge[],
  ): Promise<Memory> {
    this.ensureConnected();
    if (typeof contentOrParams === 'object') {
      const p = contentOrParams;
      return this.memoryService!.remember(
        p.content,
        p.importance,
        p.context,
        p.entityId,
        p.edges,
      );
    }
    return this.memoryService!.remember(
      contentOrParams,
      importance,
      context,
      entityId,
      edges,
    );
  }

  async get(memoryId: Buffer): Promise<Memory> {
    this.ensureConnected();
    return this.memoryService!.get(memoryId);
  }

  async recall(params: RecallParams): Promise<RecallOutput>;
  async recall(
    cue: string,
    strategies?: (string | RecallStrategyConfig)[],
    topK?: number,
    entityId?: string,
    scoringWeights?: ScoringWeights,
    cueContext?: Record<string, unknown>,
  ): Promise<RecallOutput>;
  async recall(
    cueOrParams: string | RecallParams,
    strategies?: (string | RecallStrategyConfig)[],
    topK?: number,
    entityId?: string,
    scoringWeights?: ScoringWeights,
    cueContext?: Record<string, unknown>,
  ): Promise<RecallOutput> {
    this.ensureConnected();
    if (typeof cueOrParams === 'object') {
      const p = cueOrParams;
      return this.memoryService!.recall(
        p.cue,
        p.strategies,
        p.topK,
        p.entityId,
        p.scoringWeights,
        p.cueContext,
      );
    }
    return this.memoryService!.recall(
      cueOrParams,
      strategies,
      topK,
      entityId,
      scoringWeights,
      cueContext,
    );
  }

  async prime(params: PrimeParams): Promise<PrimeOutput>;
  async prime(
    entityId: string,
    maxMemories?: number,
    similarityCue?: string,
    scoringWeights?: ScoringWeights,
  ): Promise<PrimeOutput>;
  async prime(
    entityIdOrParams: string | PrimeParams,
    maxMemories?: number,
    similarityCue?: string,
    scoringWeights?: ScoringWeights,
  ): Promise<PrimeOutput> {
    this.ensureConnected();
    if (typeof entityIdOrParams === 'object') {
      const p = entityIdOrParams;
      return this.memoryService!.prime(
        p.entityId,
        p.maxMemories,
        p.similarityCue,
        p.scoringWeights,
      );
    }
    return this.memoryService!.prime(
      entityIdOrParams,
      maxMemories,
      similarityCue,
      scoringWeights,
    );
  }

  async revise(memoryId: Buffer, params: ReviseParams): Promise<Memory> {
    this.ensureConnected();
    return this.memoryService!.revise(
      memoryId,
      params.content,
      params.importance,
      params.context,
      params.entityId,
    );
  }

  async forget(params: ForgetParams): Promise<ForgetResult> {
    this.ensureConnected();
    return this.memoryService!.forget(params.entityId, params.memoryIds);
  }

  async setPolicy(params: SetPolicyParams): Promise<boolean> {
    this.ensureConnected();
    return this.memoryService!.setPolicy(
      params.maxSnapshotsPerMemory,
      params.autoForgetThreshold,
      params.decayHalfLifeDays,
    );
  }

  // ── SubscribeService ───────────────────────────────────────────────

  async subscribe(params?: SubscribeParams): Promise<Subscription> {
    this.ensureConnected();
    return this.subscribeService!.subscribe(
      params?.entityId,
      params?.confidenceThreshold ?? 0.5,
    );
  }

  // ── ReflectService ─────────────────────────────────────────────────

  async reflect(params?: ReflectParams): Promise<ReflectResult> {
    this.ensureConnected();
    return this.reflectService!.reflect(params?.entityId);
  }

  async insights(params?: InsightsParams): Promise<Memory[]> {
    this.ensureConnected();
    return this.reflectService!.getInsights(
      params?.entityId,
      params?.maxResults,
    );
  }

  async reflectPrepare(params?: ReflectPrepareParams): Promise<ReflectPrepareResult> {
    this.ensureConnected();
    return this.reflectService!.reflectPrepare(params?.entityId);
  }

  async reflectCommit(params: ReflectCommitParams): Promise<ReflectCommitResult> {
    this.ensureConnected();
    return this.reflectService!.reflectCommit(
      params.sessionId,
      params.insights,
    );
  }

  async contradictionPrepare(): Promise<PendingContradiction[]> {
    this.ensureConnected();
    return this.reflectService!.contradictionPrepare();
  }

  async contradictionCommit(
    verdicts: ContradictionVerdictInput[],
  ): Promise<ContradictionCommitResult> {
    this.ensureConnected();
    return this.reflectService!.contradictionCommit(verdicts);
  }

  // ── HealthService ──────────────────────────────────────────────────

  async health(): Promise<HealthStatus> {
    this.ensureConnected();
    return this.healthService!.check();
  }

  async count(): Promise<number> {
    const status = await this.health();
    return status.memoryCount;
  }
}

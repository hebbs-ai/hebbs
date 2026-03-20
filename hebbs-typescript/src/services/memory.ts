/**
 * Async wrapper for the HEBBS MemoryService gRPC methods.
 */

import type { Metadata, ServiceError } from '@grpc/grpc-js';
import { mapGrpcError } from '../errors.js';
import {
  grpcUnary,
  toProtoStruct,
  protoToMemory,
  protoToRecallResult,
  edgeToProto,
  memoryKindToProto,
  scoringWeightsToProto,
  strategyConfigToProto,
  strategyToProto,
  strategyFromProto,
} from '../proto.js';
import type {
  Memory,
  MemoryKind,
  RecallOutput,
  PrimeOutput,
  ForgetResult,
  ScoringWeights,
  RecallStrategyConfig,
  Edge,
  StrategyError,
} from '../types.js';

/* eslint-disable @typescript-eslint/no-explicit-any */

export class MemoryService {
  constructor(
    private readonly stub: any,
    private readonly metadata: Metadata,
    private readonly tenantId?: string,
  ) {}

  async remember(
    content: string,
    importance?: number,
    context?: Record<string, unknown>,
    entityId?: string,
    edges?: Edge[],
    kind?: MemoryKind,
  ): Promise<Memory> {
    const req: any = { content };
    if (importance !== undefined) req.importance = importance;
    const ctx = toProtoStruct(context);
    if (ctx) req.context = ctx;
    if (entityId) req.entityId = entityId;
    if (this.tenantId) req.tenantId = this.tenantId;
    if (edges && edges.length > 0) {
      req.edges = edges.map(edgeToProto);
    }
    if (kind) req.kind = memoryKindToProto(kind);

    try {
      const resp = await grpcUnary<any>((cb) =>
        this.stub.remember(req, this.metadata, cb),
      );
      return protoToMemory(resp.memory);
    } catch (e) {
      throw mapGrpcError(e);
    }
  }

  async get(memoryId: Buffer): Promise<Memory> {
    const req: any = { memoryId };
    if (this.tenantId) req.tenantId = this.tenantId;

    try {
      const resp = await grpcUnary<any>((cb) =>
        this.stub.get(req, this.metadata, cb),
      );
      return protoToMemory(resp.memory);
    } catch (e) {
      throw mapGrpcError(e);
    }
  }

  async recall(
    cue: string,
    strategies?: (string | RecallStrategyConfig)[],
    topK?: number,
    entityId?: string,
    scoringWeights?: ScoringWeights,
    cueContext?: Record<string, unknown>,
  ): Promise<RecallOutput> {
    const stratConfigs: any[] = [];
    for (const s of strategies ?? ['similarity']) {
      if (typeof s === 'string') {
        const cfg: any = { strategyType: strategyToProto(s) };
        if (entityId) cfg.entityId = entityId;
        stratConfigs.push(cfg);
      } else {
        stratConfigs.push(strategyConfigToProto(s, entityId));
      }
    }

    const req: any = { cue, strategies: stratConfigs };
    if (topK !== undefined) req.topK = topK;
    if (this.tenantId) req.tenantId = this.tenantId;
    if (scoringWeights) {
      req.scoringWeights = scoringWeightsToProto(scoringWeights);
    }
    const ctx = toProtoStruct(cueContext);
    if (ctx) req.cueContext = ctx;

    try {
      const resp = await grpcUnary<any>((cb) =>
        this.stub.recall(req, this.metadata, cb),
      );
      const results = (resp.results ?? []).map(protoToRecallResult);
      const strategyErrors: StrategyError[] = (
        resp.strategyErrors ?? []
      ).map((se: any) => ({
        strategy: strategyFromProto(se.strategy ?? se.strategyType ?? 0),
        message: se.message ?? '',
      }));
      return { results, strategyErrors };
    } catch (e) {
      throw mapGrpcError(e);
    }
  }

  async prime(
    entityId: string,
    maxMemories?: number,
    similarityCue?: string,
    scoringWeights?: ScoringWeights,
  ): Promise<PrimeOutput> {
    const req: any = { entityId };
    if (maxMemories !== undefined) req.maxMemories = maxMemories;
    if (similarityCue) req.similarityCue = similarityCue;
    if (this.tenantId) req.tenantId = this.tenantId;
    if (scoringWeights) {
      req.scoringWeights = scoringWeightsToProto(scoringWeights);
    }

    try {
      const resp = await grpcUnary<any>((cb) =>
        this.stub.prime(req, this.metadata, cb),
      );
      return {
        results: (resp.results ?? []).map(protoToRecallResult),
        temporalCount: resp.temporalCount ?? resp.temporal_count ?? 0,
        similarityCount: resp.similarityCount ?? resp.similarity_count ?? 0,
      };
    } catch (e) {
      throw mapGrpcError(e);
    }
  }

  async revise(
    memoryId: Buffer,
    content?: string,
    importance?: number,
    context?: Record<string, unknown>,
    entityId?: string,
  ): Promise<Memory> {
    const req: any = { memoryId };
    if (content !== undefined) req.content = content;
    if (importance !== undefined) req.importance = importance;
    const ctx = toProtoStruct(context);
    if (ctx) req.context = ctx;
    if (entityId) req.entityId = entityId;
    if (this.tenantId) req.tenantId = this.tenantId;

    try {
      const resp = await grpcUnary<any>((cb) =>
        this.stub.revise(req, this.metadata, cb),
      );
      return protoToMemory(resp.memory);
    } catch (e) {
      throw mapGrpcError(e);
    }
  }

  async forget(
    entityId?: string,
    memoryIds?: Buffer[],
  ): Promise<ForgetResult> {
    const req: any = {};
    if (entityId) req.entityId = entityId;
    if (memoryIds && memoryIds.length > 0) req.memoryIds = memoryIds;
    if (this.tenantId) req.tenantId = this.tenantId;

    try {
      const resp = await grpcUnary<any>((cb) =>
        this.stub.forget(req, this.metadata, cb),
      );
      return {
        forgottenCount: resp.forgottenCount ?? resp.forgotten_count ?? 0,
        cascadeCount: resp.cascadeCount ?? resp.cascade_count ?? 0,
        tombstoneCount: resp.tombstoneCount ?? resp.tombstone_count ?? 0,
        truncated: resp.truncated ?? false,
      };
    } catch (e) {
      throw mapGrpcError(e);
    }
  }

  async setPolicy(
    maxSnapshotsPerMemory?: number,
    autoForgetThreshold?: number,
    decayHalfLifeDays?: number,
  ): Promise<boolean> {
    const req: any = {};
    if (this.tenantId) req.tenantId = this.tenantId;
    if (maxSnapshotsPerMemory !== undefined)
      req.maxSnapshotsPerMemory = maxSnapshotsPerMemory;
    if (autoForgetThreshold !== undefined)
      req.autoForgetThreshold = autoForgetThreshold;
    if (decayHalfLifeDays !== undefined)
      req.decayHalfLifeDays = decayHalfLifeDays;

    try {
      const resp = await grpcUnary<any>((cb) =>
        this.stub.setPolicy(req, this.metadata, cb),
      );
      return resp.applied ?? false;
    } catch (e) {
      throw mapGrpcError(e);
    }
  }
}

/* eslint-enable @typescript-eslint/no-explicit-any */

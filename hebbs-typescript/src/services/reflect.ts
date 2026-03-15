/**
 * Async wrapper for the HEBBS ReflectService gRPC methods.
 */

import type { Metadata } from '@grpc/grpc-js';
import { mapGrpcError } from '../errors.js';
import { grpcUnary, protoToMemory } from '../proto.js';
import type { Memory, ReflectResult, ClusterMemorySummary, ClusterPrompt, ReflectPrepareResult, ProducedInsightInput, ReflectCommitResult } from '../types.js';

/* eslint-disable @typescript-eslint/no-explicit-any */

export class ReflectService {
  constructor(
    private readonly stub: any,
    private readonly metadata: Metadata,
    private readonly tenantId?: string,
  ) {}

  async reflect(entityId?: string): Promise<ReflectResult> {
    const scope: any = {};
    if (entityId) {
      scope.entity = { entityId };
    } else {
      scope.global = {};
    }

    const req: any = { scope };
    if (this.tenantId) req.tenantId = this.tenantId;

    try {
      const resp = await grpcUnary<any>((cb) =>
        this.stub.reflect(req, this.metadata, cb),
      );
      return {
        insightsCreated: resp.insightsCreated ?? resp.insights_created ?? 0,
        clustersFound: resp.clustersFound ?? resp.clusters_found ?? 0,
        clustersProcessed:
          resp.clustersProcessed ?? resp.clusters_processed ?? 0,
        memoriesProcessed:
          resp.memoriesProcessed ?? resp.memories_processed ?? 0,
      };
    } catch (e) {
      throw mapGrpcError(e);
    }
  }

  async getInsights(
    entityId?: string,
    maxResults?: number,
  ): Promise<Memory[]> {
    const req: any = {};
    if (entityId) req.entityId = entityId;
    if (maxResults !== undefined) req.maxResults = maxResults;
    if (this.tenantId) req.tenantId = this.tenantId;

    try {
      const resp = await grpcUnary<any>((cb) =>
        this.stub.getInsights(req, this.metadata, cb),
      );
      return (resp.insights ?? []).map(protoToMemory);
    } catch (e) {
      throw mapGrpcError(e);
    }
  }

  async reflectPrepare(entityId?: string): Promise<ReflectPrepareResult> {
    const scope: any = {};
    if (entityId) {
      scope.entity = { entityId };
    } else {
      scope.global = {};
    }

    const req: any = { scope };
    if (this.tenantId) req.tenantId = this.tenantId;

    try {
      const resp = await grpcUnary<any>((cb) =>
        this.stub.reflectPrepare(req, this.metadata, cb),
      );

      const clusters: ClusterPrompt[] = (resp.clusters ?? []).map((c: any) => ({
        clusterId: c.clusterId ?? c.cluster_id ?? 0,
        memberCount: c.memberCount ?? c.member_count ?? 0,
        proposalSystemPrompt: c.proposalSystemPrompt ?? c.proposal_system_prompt ?? '',
        proposalUserPrompt: c.proposalUserPrompt ?? c.proposal_user_prompt ?? '',
        memoryIds: c.memoryIds ?? c.memory_ids ?? [],
        validationContext: c.validationContext ?? c.validation_context ?? '',
        memories: (c.memories ?? []).map((m: any): ClusterMemorySummary => ({
          memoryId: m.memoryId ?? m.memory_id ?? '',
          content: m.content ?? '',
          importance: m.importance ?? 0,
          entityId: m.entityId || m.entity_id || undefined,
          createdAt: m.createdAt ?? m.created_at ?? 0,
        })),
      }));

      return {
        sessionId: resp.sessionId ?? resp.session_id ?? '',
        memoriesProcessed: resp.memoriesProcessed ?? resp.memories_processed ?? 0,
        clusters,
        existingInsightCount: resp.existingInsightCount ?? resp.existing_insight_count ?? 0,
      };
    } catch (e) {
      throw mapGrpcError(e);
    }
  }

  async reflectCommit(
    sessionId: string,
    insights: ProducedInsightInput[],
  ): Promise<ReflectCommitResult> {
    const protoInsights = insights.map((ins) => ({
      content: ins.content,
      confidence: ins.confidence,
      sourceMemoryIds: ins.sourceMemoryIds ?? [],
      tags: ins.tags ?? [],
      ...(ins.clusterId !== undefined ? { clusterId: ins.clusterId } : {}),
    }));

    const req: any = {
      sessionId,
      insights: protoInsights,
    };
    if (this.tenantId) req.tenantId = this.tenantId;

    try {
      const resp = await grpcUnary<any>((cb) =>
        this.stub.reflectCommit(req, this.metadata, cb),
      );
      return {
        insightsCreated: resp.insightsCreated ?? resp.insights_created ?? 0,
      };
    } catch (e) {
      throw mapGrpcError(e);
    }
  }
}

/* eslint-enable @typescript-eslint/no-explicit-any */

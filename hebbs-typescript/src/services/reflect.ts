/**
 * Async wrapper for the HEBBS ReflectService gRPC methods.
 */

import type { Metadata } from '@grpc/grpc-js';
import { mapGrpcError } from '../errors.js';
import { grpcUnary, protoToMemory } from '../proto.js';
import type { Memory, ReflectResult, ClusterMemorySummary, ClusterPrompt, ReflectPrepareResult, ProducedInsightInput, ReflectCommitResult, PendingContradiction, ContradictionVerdictInput, ContradictionCommitResult } from '../types.js';

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
  async contradictionPrepare(): Promise<PendingContradiction[]> {
    const req: any = {};
    if (this.tenantId) req.tenantId = this.tenantId;

    try {
      const resp = await grpcUnary<any>((cb) =>
        this.stub.contradictionPrepare(req, this.metadata, cb),
      );
      return (resp.candidates ?? []).map((c: any): PendingContradiction => ({
        pendingId: c.pendingId ?? c.pending_id ?? '',
        memoryIdA: c.memoryIdA ?? c.memory_id_a ?? '',
        memoryIdB: c.memoryIdB ?? c.memory_id_b ?? '',
        contentASnippet: c.contentASnippet ?? c.content_a_snippet ?? '',
        contentBSnippet: c.contentBSnippet ?? c.content_b_snippet ?? '',
        classifierScore: c.classifierScore ?? c.classifier_score ?? 0,
        classifierMethod: c.classifierMethod ?? c.classifier_method ?? '',
        similarity: c.similarity ?? 0,
        createdAt: c.createdAt ?? c.created_at ?? 0,
      }));
    } catch (e) {
      throw mapGrpcError(e);
    }
  }

  async contradictionCommit(
    verdicts: ContradictionVerdictInput[],
  ): Promise<ContradictionCommitResult> {
    const protoVerdicts = verdicts.map((v) => ({
      pendingId: v.pendingId,
      verdict: v.verdict,
      confidence: v.confidence,
      ...(v.reasoning !== undefined ? { reasoning: v.reasoning } : {}),
    }));

    const req: any = { verdicts: protoVerdicts };
    if (this.tenantId) req.tenantId = this.tenantId;

    try {
      const resp = await grpcUnary<any>((cb) =>
        this.stub.contradictionCommit(req, this.metadata, cb),
      );
      return {
        contradictionsConfirmed: resp.contradictionsConfirmed ?? resp.contradictions_confirmed ?? 0,
        revisionsCreated: resp.revisionsCreated ?? resp.revisions_created ?? 0,
        dismissed: resp.dismissed ?? 0,
      };
    } catch (e) {
      throw mapGrpcError(e);
    }
  }
}

/* eslint-enable @typescript-eslint/no-explicit-any */

/**
 * Proto loading and type-conversion utilities.
 *
 * Loads the HEBBS proto definition at runtime using @grpc/proto-loader
 * and exposes service constructors + Struct conversion helpers.
 */

import * as grpc from '@grpc/grpc-js';
import * as protoLoader from '@grpc/proto-loader';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  type Edge,
  EdgeType,
  type Memory,
  MemoryKind,
  type RecallResult,
  type ScoringWeights,
  type StrategyDetail,
  type RecallStrategyConfig,
} from './types.js';

// ── Proto Loading ──────────────────────────────────────────────────────

const currentDir = dirname(fileURLToPath(import.meta.url));
const PROTO_PATH = join(currentDir, '..', 'proto', 'hebbs.proto');

const packageDefinition = protoLoader.loadSync(PROTO_PATH, {
  keepCase: false,
  longs: Number,
  enums: Number,
  defaults: false,
  oneofs: false,
});

const descriptor = grpc.loadPackageDefinition(packageDefinition);
const hebbsV1 = (descriptor.hebbs as Record<string, unknown>)['v1'] as Record<
  string,
  grpc.ServiceClientConstructor
>;

export const MemoryServiceClient = hebbsV1['MemoryService']!;
export const SubscribeServiceClient = hebbsV1['SubscribeService']!;
export const ReflectServiceClient = hebbsV1['ReflectService']!;
export const HealthServiceClient = hebbsV1['HealthService']!;

// ── Proto Enum Constants ───────────────────────────────────────────────

export const MEMORY_KIND_UNSPECIFIED = 0;
export const MEMORY_KIND_EPISODE = 1;
export const MEMORY_KIND_INSIGHT = 2;
export const MEMORY_KIND_REVISION = 3;
export const MEMORY_KIND_DOCUMENT = 4;
export const MEMORY_KIND_PROPOSITION = 5;

export const EDGE_TYPE_UNSPECIFIED = 0;
export const EDGE_TYPE_CAUSED_BY = 1;
export const EDGE_TYPE_RELATED_TO = 2;
export const EDGE_TYPE_FOLLOWED_BY = 3;
export const EDGE_TYPE_REVISED_FROM = 4;
export const EDGE_TYPE_INSIGHT_FROM = 5;
export const EDGE_TYPE_CONTRADICTS = 6;
export const EDGE_TYPE_HAS_ENTITY = 7;
export const EDGE_TYPE_ENTITY_RELATION = 8;
export const EDGE_TYPE_PROPOSITION_OF = 9;

export const STRATEGY_UNSPECIFIED = 0;
export const STRATEGY_SIMILARITY = 1;
export const STRATEGY_TEMPORAL = 2;
export const STRATEGY_CAUSAL = 3;
export const STRATEGY_ANALOGICAL = 4;

export const SERVING_STATUS_SERVING = 1;

// ── Enum Conversion Maps ───────────────────────────────────────────────

const EDGE_TYPE_TO_PROTO: Record<string, number> = {
  [EdgeType.CAUSED_BY]: EDGE_TYPE_CAUSED_BY,
  [EdgeType.RELATED_TO]: EDGE_TYPE_RELATED_TO,
  [EdgeType.FOLLOWED_BY]: EDGE_TYPE_FOLLOWED_BY,
  [EdgeType.REVISED_FROM]: EDGE_TYPE_REVISED_FROM,
  [EdgeType.INSIGHT_FROM]: EDGE_TYPE_INSIGHT_FROM,
  [EdgeType.CONTRADICTS]: EDGE_TYPE_CONTRADICTS,
  [EdgeType.HAS_ENTITY]: EDGE_TYPE_HAS_ENTITY,
  [EdgeType.ENTITY_RELATION]: EDGE_TYPE_ENTITY_RELATION,
  [EdgeType.PROPOSITION_OF]: EDGE_TYPE_PROPOSITION_OF,
};

const STRATEGY_TO_PROTO: Record<string, number> = {
  similarity: STRATEGY_SIMILARITY,
  temporal: STRATEGY_TEMPORAL,
  causal: STRATEGY_CAUSAL,
  analogical: STRATEGY_ANALOGICAL,
};

const MEMORY_KIND_FROM_PROTO: Record<number, MemoryKind> = {
  [MEMORY_KIND_EPISODE]: MemoryKind.EPISODE,
  [MEMORY_KIND_INSIGHT]: MemoryKind.INSIGHT,
  [MEMORY_KIND_REVISION]: MemoryKind.REVISION,
  [MEMORY_KIND_DOCUMENT]: MemoryKind.DOCUMENT,
  [MEMORY_KIND_PROPOSITION]: MemoryKind.PROPOSITION,
};

const MEMORY_KIND_TO_PROTO: Record<string, number> = {
  [MemoryKind.EPISODE]: MEMORY_KIND_EPISODE,
  [MemoryKind.INSIGHT]: MEMORY_KIND_INSIGHT,
  [MemoryKind.REVISION]: MEMORY_KIND_REVISION,
  [MemoryKind.DOCUMENT]: MEMORY_KIND_DOCUMENT,
  [MemoryKind.PROPOSITION]: MEMORY_KIND_PROPOSITION,
};

const STRATEGY_FROM_PROTO: Record<number, string> = {
  [STRATEGY_SIMILARITY]: 'similarity',
  [STRATEGY_TEMPORAL]: 'temporal',
  [STRATEGY_CAUSAL]: 'causal',
  [STRATEGY_ANALOGICAL]: 'analogical',
};

// ── gRPC Call Helper ───────────────────────────────────────────────────

/**
 * Promisify a gRPC unary call. Binds the request and metadata to the
 * callback-based stub method and returns a Promise.
 */
export function grpcUnary<T>(
  fn: (callback: (err: grpc.ServiceError | null, res: T) => void) => void,
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    fn((err, res) => {
      if (err) reject(err);
      else resolve(res);
    });
  });
}

// ── Struct Conversion ──────────────────────────────────────────────────

/* eslint-disable @typescript-eslint/no-explicit-any */

function toProtoValue(value: unknown): any {
  if (value === null || value === undefined) return { nullValue: 0 };
  if (typeof value === 'number') return { numberValue: value };
  if (typeof value === 'string') return { stringValue: value };
  if (typeof value === 'boolean') return { boolValue: value };
  if (Array.isArray(value))
    return { listValue: { values: value.map(toProtoValue) } };
  if (typeof value === 'object')
    return { structValue: toProtoStruct(value as Record<string, unknown>) };
  return { stringValue: String(value) };
}

export function toProtoStruct(
  obj: Record<string, unknown> | undefined,
): any | undefined {
  if (!obj || Object.keys(obj).length === 0) return undefined;
  const fields: Record<string, any> = {};
  for (const [key, value] of Object.entries(obj)) {
    fields[key] = toProtoValue(value);
  }
  return { fields };
}

function fromProtoValue(value: any): unknown {
  if (!value) return null;
  if ('nullValue' in value) return null;
  if ('numberValue' in value) return value.numberValue;
  if ('stringValue' in value) return value.stringValue;
  if ('boolValue' in value) return value.boolValue;
  if ('listValue' in value)
    return (value.listValue?.values ?? []).map((v: any) => fromProtoValue(v));
  if ('structValue' in value) return fromProtoStruct(value.structValue);
  return null;
}

export function fromProtoStruct(struct: any): Record<string, unknown> {
  if (!struct || !struct.fields) return {};
  const result: Record<string, unknown> = {};
  for (const [key, protoVal] of Object.entries(struct.fields)) {
    result[key] = fromProtoValue(protoVal);
  }
  return result;
}

// ── Type Conversion (Proto → SDK) ──────────────────────────────────────

export function protoToMemory(m: any): Memory {
  return {
    id: Buffer.from(m.memoryId ?? m.memory_id ?? []),
    content: m.content ?? '',
    importance: m.importance ?? 0,
    context: fromProtoStruct(m.context),
    entityId: m.entityId || m.entity_id || undefined,
    createdAt: m.createdAt ?? m.created_at ?? 0,
    updatedAt: m.updatedAt ?? m.updated_at ?? 0,
    lastAccessedAt: m.lastAccessedAt ?? m.last_accessed_at ?? 0,
    accessCount: m.accessCount ?? m.access_count ?? 0,
    decayScore: m.decayScore ?? m.decay_score ?? 0,
    kind:
      MEMORY_KIND_FROM_PROTO[m.kind ?? MEMORY_KIND_UNSPECIFIED] ??
      MemoryKind.UNSPECIFIED,
    embedding: m.embedding ? Array.from(m.embedding) : [],
    sourceMemoryIds: (m.sourceMemoryIds ?? m.source_memory_ids ?? []).map(
      (b: any) => Buffer.from(b),
    ),
  };
}

export function protoToStrategyDetail(sd: any): StrategyDetail {
  const stratType = sd.strategyType ?? sd.strategy_type ?? 0;
  return {
    strategy: STRATEGY_FROM_PROTO[stratType] ?? 'unknown',
    relevance: sd.relevance ?? 0,
    distance: sd.distance !== undefined && sd.distance !== 0 ? sd.distance : undefined,
    timestamp: sd.timestamp !== undefined && sd.timestamp !== 0 ? sd.timestamp : undefined,
    rank: sd.rank !== undefined ? sd.rank : undefined,
    depth: sd.depth !== undefined && sd.depth !== 0 ? sd.depth : undefined,
    embeddingSimilarity:
      sd.embeddingSimilarity !== undefined && sd.embeddingSimilarity !== 0
        ? sd.embeddingSimilarity
        : sd.embedding_similarity !== undefined && sd.embedding_similarity !== 0
          ? sd.embedding_similarity
          : undefined,
    structuralSimilarity:
      sd.structuralSimilarity !== undefined && sd.structuralSimilarity !== 0
        ? sd.structuralSimilarity
        : sd.structural_similarity !== undefined && sd.structural_similarity !== 0
          ? sd.structural_similarity
          : undefined,
  };
}

export function protoToRecallResult(r: any): RecallResult {
  const details = r.strategyDetails ?? r.strategy_details ?? [];
  return {
    memory: protoToMemory(r.memory),
    score: r.score ?? 0,
    strategyDetails: details.map(protoToStrategyDetail),
  };
}

// ── Type Conversion (SDK → Proto) ──────────────────────────────────────

export function edgeToProto(edge: Edge): any {
  const proto: any = {
    targetId: edge.targetId,
    edgeType: EDGE_TYPE_TO_PROTO[edge.edgeType] ?? EDGE_TYPE_UNSPECIFIED,
  };
  if (edge.confidence !== undefined) {
    proto.confidence = edge.confidence;
  }
  return proto;
}

export function scoringWeightsToProto(
  sw: ScoringWeights,
): any {
  const proto: any = {
    wRelevance: sw.wRelevance ?? 0.5,
    wRecency: sw.wRecency ?? 0.2,
    wImportance: sw.wImportance ?? 0.2,
    wReinforcement: sw.wReinforcement ?? 0.1,
  };
  if (sw.maxAgeUs !== undefined) proto.maxAgeUs = sw.maxAgeUs;
  if (sw.reinforcementCap !== undefined)
    proto.reinforcementCap = sw.reinforcementCap;
  return proto;
}

export function strategyConfigToProto(
  sc: RecallStrategyConfig,
  fallbackEntityId?: string,
): any {
  const proto: any = {
    strategyType: STRATEGY_TO_PROTO[sc.strategy] ?? STRATEGY_UNSPECIFIED,
  };
  const eid = sc.entityId ?? fallbackEntityId;
  if (eid) proto.entityId = eid;
  if (sc.topK !== undefined) proto.topK = sc.topK;
  if (sc.efSearch !== undefined) proto.efSearch = sc.efSearch;
  if (sc.timeRange) {
    proto.timeRange = { startUs: sc.timeRange[0], endUs: sc.timeRange[1] };
  }
  if (sc.seedMemoryId) proto.seedMemoryId = sc.seedMemoryId;
  if (sc.edgeTypes) {
    proto.edgeTypes = sc.edgeTypes.map(
      (et) => EDGE_TYPE_TO_PROTO[et] ?? EDGE_TYPE_UNSPECIFIED,
    );
  }
  if (sc.maxDepth !== undefined) proto.maxDepth = sc.maxDepth;
  if (sc.analogicalAlpha !== undefined)
    proto.analogicalAlpha = sc.analogicalAlpha;
  return proto;
}

export function strategyToProto(s: string): number {
  return STRATEGY_TO_PROTO[s] ?? STRATEGY_UNSPECIFIED;
}

export function memoryKindToProto(kind: MemoryKind): number {
  return MEMORY_KIND_TO_PROTO[kind] ?? MEMORY_KIND_UNSPECIFIED;
}

export function strategyFromProto(n: number): string {
  return STRATEGY_FROM_PROTO[n] ?? 'unknown';
}

/* eslint-enable @typescript-eslint/no-explicit-any */

/**
 * Public data types for the HEBBS TypeScript SDK.
 *
 * All types are plain interfaces and enums -- no protobuf leakage in the public API.
 */

// ── Enums ──────────────────────────────────────────────────────────────

export enum MemoryKind {
  EPISODE = 'episode',
  INSIGHT = 'insight',
  REVISION = 'revision',
  UNSPECIFIED = 'unspecified',
}

export enum EdgeType {
  CAUSED_BY = 'caused_by',
  RELATED_TO = 'related_to',
  FOLLOWED_BY = 'followed_by',
  REVISED_FROM = 'revised_from',
  INSIGHT_FROM = 'insight_from',
  CONTRADICTS = 'contradicts',
  UNSPECIFIED = 'unspecified',
}

export enum RecallStrategy {
  SIMILARITY = 'similarity',
  TEMPORAL = 'temporal',
  CAUSAL = 'causal',
  ANALOGICAL = 'analogical',
}

// ── Core Types ─────────────────────────────────────────────────────────

export interface Edge {
  readonly targetId: Buffer;
  readonly edgeType: EdgeType;
  readonly confidence?: number;
}

export interface Memory {
  readonly id: Buffer;
  readonly content: string;
  readonly importance: number;
  readonly context: Record<string, unknown>;
  readonly entityId?: string;
  readonly createdAt: number;
  readonly updatedAt: number;
  readonly lastAccessedAt: number;
  readonly accessCount: number;
  readonly decayScore: number;
  readonly kind: MemoryKind;
  readonly embedding: number[];
  readonly sourceMemoryIds: Buffer[];
}

// ── Recall Types ───────────────────────────────────────────────────────

export interface StrategyDetail {
  readonly strategy: string;
  readonly relevance: number;
  readonly distance?: number;
  readonly timestamp?: number;
  readonly rank?: number;
  readonly depth?: number;
  readonly embeddingSimilarity?: number;
  readonly structuralSimilarity?: number;
}

export interface RecallResult {
  readonly memory: Memory;
  readonly score: number;
  readonly strategyDetails: StrategyDetail[];
}

export interface StrategyError {
  readonly strategy: string;
  readonly message: string;
}

export interface RecallOutput {
  readonly results: RecallResult[];
  readonly strategyErrors: StrategyError[];
}

/**
 * Per-strategy configuration for advanced recall tuning.
 *
 * Most users should just pass strategy names as strings:
 * ```
 * strategies: ['similarity', 'temporal']
 * ```
 *
 * Use this interface when you need strategy-specific parameters:
 * ```
 * strategies: [{ strategy: 'causal', seedMemoryId: mem.id, maxDepth: 3 }]
 * ```
 */
export interface RecallStrategyConfig {
  readonly strategy: string;
  readonly entityId?: string;
  readonly topK?: number;
  readonly efSearch?: number;
  readonly timeRange?: [number, number];
  readonly seedMemoryId?: Buffer;
  readonly edgeTypes?: EdgeType[];
  readonly maxDepth?: number;
  readonly analogicalAlpha?: number;
}

// ── Scoring ────────────────────────────────────────────────────────────

/**
 * Composite scoring weight overrides for recall and prime operations.
 *
 * When omitted, the engine uses default weights:
 * wRelevance=0.5, wRecency=0.2, wImportance=0.2, wReinforcement=0.1.
 */
export interface ScoringWeights {
  readonly wRelevance?: number;
  readonly wRecency?: number;
  readonly wImportance?: number;
  readonly wReinforcement?: number;
  readonly maxAgeUs?: number;
  readonly reinforcementCap?: number;
}

// ── Operation Results ──────────────────────────────────────────────────

export interface PrimeOutput {
  readonly results: RecallResult[];
  readonly temporalCount: number;
  readonly similarityCount: number;
}

export interface ForgetResult {
  readonly forgottenCount: number;
  readonly cascadeCount: number;
  readonly tombstoneCount: number;
  readonly truncated: boolean;
}

export interface ReflectResult {
  readonly insightsCreated: number;
  readonly clustersFound: number;
  readonly clustersProcessed: number;
  readonly memoriesProcessed: number;
}

export interface SubscribePush {
  readonly subscriptionId: number;
  readonly memory: Memory;
  readonly confidence: number;
  readonly pushTimestampUs: number;
  readonly sequenceNumber: number;
}

export interface HealthStatus {
  readonly serving: boolean;
  readonly version: string;
  readonly memoryCount: number;
  readonly uptimeSeconds: number;
}

// ── Method Parameter Types ─────────────────────────────────────────────

export interface RememberParams {
  readonly content: string;
  readonly importance?: number;
  readonly context?: Record<string, unknown>;
  readonly entityId?: string;
  readonly edges?: Edge[];
}

export interface RecallParams {
  readonly cue: string;
  readonly strategies?: (string | RecallStrategyConfig)[];
  readonly topK?: number;
  readonly entityId?: string;
  readonly scoringWeights?: ScoringWeights;
  readonly cueContext?: Record<string, unknown>;
}

export interface PrimeParams {
  readonly entityId: string;
  readonly maxMemories?: number;
  readonly similarityCue?: string;
  readonly scoringWeights?: ScoringWeights;
}

export interface ReviseParams {
  readonly content?: string;
  readonly importance?: number;
  readonly context?: Record<string, unknown>;
  readonly entityId?: string;
}

export interface ForgetParams {
  readonly entityId?: string;
  readonly memoryIds?: Buffer[];
}

export interface SetPolicyParams {
  readonly maxSnapshotsPerMemory?: number;
  readonly autoForgetThreshold?: number;
  readonly decayHalfLifeDays?: number;
}

export interface SubscribeParams {
  readonly entityId?: string;
  readonly confidenceThreshold?: number;
}

export interface ReflectParams {
  readonly entityId?: string;
}

export interface InsightsParams {
  readonly entityId?: string;
  readonly maxResults?: number;
}

export interface ReflectPrepareParams {
  readonly entityId?: string;
}

export interface ReflectCommitParams {
  readonly sessionId: string;
  readonly insights: ProducedInsightInput[];
}

// ── Reflect Prepare/Commit Types ────────────────────────────────────

export interface ClusterMemorySummary {
  readonly memoryId: string;
  readonly content: string;
  readonly importance: number;
  readonly entityId?: string;
  readonly createdAt: number;
}

export interface ClusterPrompt {
  readonly clusterId: number;
  readonly memberCount: number;
  readonly proposalSystemPrompt: string;
  readonly proposalUserPrompt: string;
  readonly memoryIds: string[];
  readonly validationContext: string;
  readonly memories: ClusterMemorySummary[];
}

export interface ReflectPrepareResult {
  readonly sessionId: string;
  readonly memoriesProcessed: number;
  readonly clusters: ClusterPrompt[];
  readonly existingInsightCount: number;
}

export interface ProducedInsightInput {
  readonly content: string;
  readonly confidence: number;
  readonly sourceMemoryIds?: string[];
  readonly tags?: string[];
  readonly clusterId?: number;
}

export interface ReflectCommitResult {
  readonly insightsCreated: number;
}

// ── Contradiction Prepare/Commit Types ─────────────────────────────

export interface PendingContradiction {
  readonly pendingId: string;
  readonly memoryIdA: string;
  readonly memoryIdB: string;
  readonly contentASnippet: string;
  readonly contentBSnippet: string;
  readonly classifierScore: number;
  readonly classifierMethod: string;
  readonly similarity: number;
  readonly createdAt: number;
}

export interface ContradictionVerdictInput {
  readonly pendingId: string;
  readonly verdict: 'contradiction' | 'revision' | 'dismiss';
  readonly confidence: number;
  readonly reasoning?: string;
}

export interface ContradictionCommitResult {
  readonly contradictionsConfirmed: number;
  readonly revisionsCreated: number;
  readonly dismissed: number;
}

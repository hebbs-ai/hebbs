import { describe, it, expect } from 'vitest';
import {
  MemoryKind,
  EdgeType,
  RecallStrategy,
  type Memory,
  type Edge,
  type RecallResult,
  type RecallOutput,
  type PrimeOutput,
  type ScoringWeights,
  type ForgetResult,
  type ReflectResult,
  type SubscribePush,
  type HealthStatus,
  type RecallStrategyConfig,
  type StrategyDetail,
  type StrategyError,
  type ClusterMemorySummary,
  type ClusterPrompt,
  type ProducedInsightInput,
  type ReflectCommitResult,
  type ReflectPrepareResult,
  type PendingContradiction,
  type ContradictionVerdictInput,
  type ContradictionCommitResult,
} from '../../src/index.js';

describe('MemoryKind', () => {
  it('has all expected values', () => {
    expect(MemoryKind.EPISODE).toBe('episode');
    expect(MemoryKind.INSIGHT).toBe('insight');
    expect(MemoryKind.REVISION).toBe('revision');
    expect(MemoryKind.UNSPECIFIED).toBe('unspecified');
  });
});

describe('EdgeType', () => {
  it('has all expected values', () => {
    expect(EdgeType.CAUSED_BY).toBe('caused_by');
    expect(EdgeType.RELATED_TO).toBe('related_to');
    expect(EdgeType.FOLLOWED_BY).toBe('followed_by');
    expect(EdgeType.REVISED_FROM).toBe('revised_from');
    expect(EdgeType.INSIGHT_FROM).toBe('insight_from');
    expect(EdgeType.CONTRADICTS).toBe('contradicts');
    expect(EdgeType.UNSPECIFIED).toBe('unspecified');
  });
});

describe('RecallStrategy', () => {
  it('has all expected values', () => {
    expect(RecallStrategy.SIMILARITY).toBe('similarity');
    expect(RecallStrategy.TEMPORAL).toBe('temporal');
    expect(RecallStrategy.CAUSAL).toBe('causal');
    expect(RecallStrategy.ANALOGICAL).toBe('analogical');
  });
});

describe('Memory interface', () => {
  it('can construct a full Memory object', () => {
    const mem: Memory = {
      id: Buffer.from('abc123', 'hex'),
      content: 'Test memory',
      importance: 0.8,
      context: { key: 'value' },
      entityId: 'test-entity',
      createdAt: 1000000,
      updatedAt: 1000001,
      lastAccessedAt: 1000002,
      accessCount: 5,
      decayScore: 0.95,
      kind: MemoryKind.EPISODE,
      embedding: [0.1, 0.2, 0.3],
    };

    expect(mem.id).toBeInstanceOf(Buffer);
    expect(mem.content).toBe('Test memory');
    expect(mem.importance).toBe(0.8);
    expect(mem.context).toEqual({ key: 'value' });
    expect(mem.entityId).toBe('test-entity');
    expect(mem.kind).toBe(MemoryKind.EPISODE);
    expect(mem.embedding).toHaveLength(3);
  });

  it('can construct a minimal Memory', () => {
    const mem: Memory = {
      id: Buffer.alloc(16),
      content: '',
      importance: 0,
      context: {},
      createdAt: 0,
      updatedAt: 0,
      lastAccessedAt: 0,
      accessCount: 0,
      decayScore: 0,
      kind: MemoryKind.UNSPECIFIED,
      embedding: [],
    };

    expect(mem.entityId).toBeUndefined();
    expect(mem.accessCount).toBe(0);
  });
});

describe('Edge interface', () => {
  it('can construct an Edge with confidence', () => {
    const edge: Edge = {
      targetId: Buffer.from('abc123', 'hex'),
      edgeType: EdgeType.FOLLOWED_BY,
      confidence: 0.95,
    };

    expect(edge.targetId).toBeInstanceOf(Buffer);
    expect(edge.edgeType).toBe(EdgeType.FOLLOWED_BY);
    expect(edge.confidence).toBe(0.95);
  });

  it('can construct an Edge without confidence', () => {
    const edge: Edge = {
      targetId: Buffer.alloc(16),
      edgeType: EdgeType.CAUSED_BY,
    };

    expect(edge.confidence).toBeUndefined();
  });
});

describe('ScoringWeights interface', () => {
  it('can construct with all fields', () => {
    const sw: ScoringWeights = {
      wRelevance: 0.5,
      wRecency: 0.2,
      wImportance: 0.2,
      wReinforcement: 0.1,
      maxAgeUs: 86400_000_000,
      reinforcementCap: 100,
    };

    expect(sw.wRelevance).toBe(0.5);
    expect(sw.maxAgeUs).toBe(86400_000_000);
  });

  it('works with partial fields', () => {
    const sw: ScoringWeights = {
      wRelevance: 0.9,
    };

    expect(sw.wRelevance).toBe(0.9);
    expect(sw.wRecency).toBeUndefined();
  });
});

describe('RecallStrategyConfig interface', () => {
  it('supports similarity config', () => {
    const cfg: RecallStrategyConfig = {
      strategy: 'similarity',
      topK: 10,
      efSearch: 64,
    };

    expect(cfg.strategy).toBe('similarity');
    expect(cfg.topK).toBe(10);
  });

  it('supports causal config', () => {
    const cfg: RecallStrategyConfig = {
      strategy: 'causal',
      seedMemoryId: Buffer.from('seed', 'utf-8'),
      maxDepth: 3,
      edgeTypes: [EdgeType.FOLLOWED_BY, EdgeType.CAUSED_BY],
    };

    expect(cfg.edgeTypes).toHaveLength(2);
    expect(cfg.maxDepth).toBe(3);
  });

  it('supports analogical config', () => {
    const cfg: RecallStrategyConfig = {
      strategy: 'analogical',
      analogicalAlpha: 0.7,
    };

    expect(cfg.analogicalAlpha).toBe(0.7);
  });

  it('supports temporal config with time range', () => {
    const cfg: RecallStrategyConfig = {
      strategy: 'temporal',
      entityId: 'test',
      timeRange: [1000, 2000],
    };

    expect(cfg.timeRange).toEqual([1000, 2000]);
  });
});

describe('RecallOutput interface', () => {
  it('can hold results and errors', () => {
    const detail: StrategyDetail = {
      strategy: 'similarity',
      relevance: 0.85,
      distance: 0.15,
    };

    const result: RecallResult = {
      memory: {
        id: Buffer.alloc(16),
        content: 'test',
        importance: 0.5,
        context: {},
        createdAt: 0,
        updatedAt: 0,
        lastAccessedAt: 0,
        accessCount: 0,
        decayScore: 0,
        kind: MemoryKind.EPISODE,
        embedding: [],
      },
      score: 0.9,
      strategyDetails: [detail],
    };

    const error: StrategyError = {
      strategy: 'causal',
      message: 'no seed memory found',
    };

    const output: RecallOutput = {
      results: [result],
      strategyErrors: [error],
    };

    expect(output.results).toHaveLength(1);
    expect(output.results[0].score).toBe(0.9);
    expect(output.strategyErrors).toHaveLength(1);
    expect(output.strategyErrors[0].strategy).toBe('causal');
  });
});

describe('ForgetResult interface', () => {
  it('captures all fields', () => {
    const result: ForgetResult = {
      forgottenCount: 5,
      cascadeCount: 2,
      tombstoneCount: 5,
      truncated: false,
    };

    expect(result.forgottenCount).toBe(5);
    expect(result.truncated).toBe(false);
  });
});

describe('ReflectResult interface', () => {
  it('captures all fields', () => {
    const result: ReflectResult = {
      insightsCreated: 3,
      clustersFound: 5,
      clustersProcessed: 4,
      memoriesProcessed: 20,
    };

    expect(result.insightsCreated).toBe(3);
    expect(result.memoriesProcessed).toBe(20);
  });
});

describe('SubscribePush interface', () => {
  it('captures all fields', () => {
    const push: SubscribePush = {
      subscriptionId: 42,
      memory: {
        id: Buffer.alloc(16),
        content: 'pushed memory',
        importance: 0.7,
        context: {},
        createdAt: 100,
        updatedAt: 100,
        lastAccessedAt: 100,
        accessCount: 0,
        decayScore: 1.0,
        kind: MemoryKind.EPISODE,
        embedding: [],
      },
      confidence: 0.85,
      pushTimestampUs: 1234567890,
      sequenceNumber: 1,
    };

    expect(push.subscriptionId).toBe(42);
    expect(push.confidence).toBe(0.85);
    expect(push.memory.content).toBe('pushed memory');
  });
});

describe('HealthStatus interface', () => {
  it('captures all fields', () => {
    const status: HealthStatus = {
      serving: true,
      version: '0.1.0',
      memoryCount: 1000,
      uptimeSeconds: 3600,
    };

    expect(status.serving).toBe(true);
    expect(status.memoryCount).toBe(1000);
  });
});

describe('PrimeOutput interface', () => {
  it('captures counts and results', () => {
    const output: PrimeOutput = {
      results: [],
      temporalCount: 5,
      similarityCount: 3,
    };

    expect(output.temporalCount).toBe(5);
    expect(output.similarityCount).toBe(3);
  });
});

describe('ReflectPrepareResult interface', () => {
  it('can construct a full result', () => {
    const result: ReflectPrepareResult = {
      sessionId: 'sess-123',
      memoriesProcessed: 42,
      clusters: [
        {
          clusterId: 0,
          memberCount: 5,
          proposalSystemPrompt: 'You are...',
          proposalUserPrompt: 'Analyze...',
          memoryIds: ['m1', 'm2'],
          validationContext: 'context',
          memories: [
            {
              memoryId: 'm1',
              content: 'Test memory',
              importance: 0.8,
              entityId: 'ent-1',
              createdAt: 1000,
            },
          ],
        },
      ],
      existingInsightCount: 3,
    };

    expect(result.sessionId).toBe('sess-123');
    expect(result.memoriesProcessed).toBe(42);
    expect(result.clusters).toHaveLength(1);
    expect(result.clusters[0].memberCount).toBe(5);
    expect(result.clusters[0].memories).toHaveLength(1);
    expect(result.clusters[0].memories[0].content).toBe('Test memory');
    expect(result.existingInsightCount).toBe(3);
  });
});

describe('ProducedInsightInput interface', () => {
  it('can construct with all fields', () => {
    const insight: ProducedInsightInput = {
      content: 'Users prefer API-first',
      confidence: 0.85,
      sourceMemoryIds: ['m1', 'm2', 'm3'],
      tags: ['preference', 'api'],
      clusterId: 2,
    };

    expect(insight.content).toBe('Users prefer API-first');
    expect(insight.confidence).toBe(0.85);
    expect(insight.sourceMemoryIds).toHaveLength(3);
    expect(insight.clusterId).toBe(2);
  });

  it('works with minimal fields', () => {
    const insight: ProducedInsightInput = {
      content: 'test',
      confidence: 0.5,
    };

    expect(insight.sourceMemoryIds).toBeUndefined();
    expect(insight.tags).toBeUndefined();
    expect(insight.clusterId).toBeUndefined();
  });
});

describe('ReflectCommitResult interface', () => {
  it('captures insights created', () => {
    const result: ReflectCommitResult = {
      insightsCreated: 5,
    };
    expect(result.insightsCreated).toBe(5);
  });
});

describe('Edge with CONTRADICTS type', () => {
  it('supports CONTRADICTS edge type', () => {
    const edge: Edge = {
      targetId: Buffer.alloc(16),
      edgeType: EdgeType.CONTRADICTS,
      confidence: 0.75,
    };

    expect(edge.edgeType).toBe(EdgeType.CONTRADICTS);
    expect(edge.confidence).toBe(0.75);
  });
});

describe('PendingContradiction interface', () => {
  it('can construct a full PendingContradiction', () => {
    const pc: PendingContradiction = {
      pendingId: 'abc123',
      memoryIdA: 'mem_a',
      memoryIdB: 'mem_b',
      contentASnippet: 'The system is reliable',
      contentBSnippet: 'The system is unreliable',
      classifierScore: 0.65,
      classifierMethod: 'heuristic',
      similarity: 0.82,
      createdAt: 1_700_000_000_000_000,
    };

    expect(pc.pendingId).toBe('abc123');
    expect(pc.memoryIdA).toBe('mem_a');
    expect(pc.memoryIdB).toBe('mem_b');
    expect(pc.contentASnippet).toBe('The system is reliable');
    expect(pc.contentBSnippet).toBe('The system is unreliable');
    expect(pc.classifierScore).toBe(0.65);
    expect(pc.classifierMethod).toBe('heuristic');
    expect(pc.similarity).toBe(0.82);
    expect(pc.createdAt).toBe(1_700_000_000_000_000);
  });
});

describe('ContradictionVerdictInput interface', () => {
  it('can construct a contradiction verdict', () => {
    const v: ContradictionVerdictInput = {
      pendingId: 'abc123',
      verdict: 'contradiction',
      confidence: 0.9,
      reasoning: 'Direct conflict in vendor assessment',
    };

    expect(v.pendingId).toBe('abc123');
    expect(v.verdict).toBe('contradiction');
    expect(v.confidence).toBe(0.9);
    expect(v.reasoning).toBe('Direct conflict in vendor assessment');
  });

  it('can construct a dismiss verdict without reasoning', () => {
    const v: ContradictionVerdictInput = {
      pendingId: 'def456',
      verdict: 'dismiss',
      confidence: 0.95,
    };

    expect(v.verdict).toBe('dismiss');
    expect(v.reasoning).toBeUndefined();
  });

  it('can construct a revision verdict', () => {
    const v: ContradictionVerdictInput = {
      pendingId: 'ghi789',
      verdict: 'revision',
      confidence: 0.85,
      reasoning: 'Updated timeline',
    };

    expect(v.verdict).toBe('revision');
  });
});

describe('ContradictionCommitResult interface', () => {
  it('captures all fields', () => {
    const result: ContradictionCommitResult = {
      contradictionsConfirmed: 2,
      revisionsCreated: 1,
      dismissed: 3,
    };

    expect(result.contradictionsConfirmed).toBe(2);
    expect(result.revisionsCreated).toBe(1);
    expect(result.dismissed).toBe(3);
  });
});

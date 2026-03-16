/**
 * HEBBS TypeScript SDK -- async gRPC client for the HEBBS cognitive memory engine.
 */

export { HebbsClient, type HebbsClientOptions } from './client.js';
export { Subscription } from './services/subscribe.js';

export {
  MemoryKind,
  EdgeType,
  RecallStrategy,
  type Edge,
  type Memory,
  type StrategyDetail,
  type RecallResult,
  type StrategyError,
  type RecallOutput,
  type RecallStrategyConfig,
  type ScoringWeights,
  type PrimeOutput,
  type ForgetResult,
  type ReflectResult,
  type SubscribePush,
  type HealthStatus,
  type RememberParams,
  type RecallParams,
  type PrimeParams,
  type ReviseParams,
  type ForgetParams,
  type SetPolicyParams,
  type SubscribeParams,
  type ReflectParams,
  type InsightsParams,
  type ClusterMemorySummary,
  type ClusterPrompt,
  type ProducedInsightInput,
  type ReflectCommitResult,
  type ReflectPrepareResult,
  type ReflectPrepareParams,
  type ReflectCommitParams,
  type PendingContradiction,
  type ContradictionVerdictInput,
  type ContradictionCommitResult,
} from './types.js';

export {
  HebbsError,
  HebbsConnectionError,
  HebbsTimeoutError,
  HebbsNotFoundError,
  HebbsUnavailableError,
  HebbsInvalidArgumentError,
  HebbsInternalError,
  HebbsAuthenticationError,
  HebbsPermissionDeniedError,
  HebbsRateLimitError,
} from './errors.js';

export const VERSION = '0.3.0';

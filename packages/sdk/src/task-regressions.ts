import type { DaemonClient } from './client.js';

/** Synthetic actor inputs only. The private oracle is never actor context. */
export interface RegressionActorFixture {
  prompt: string;
  initialFiles: Record<string, string>;
  readableFiles: string[];
  writableFiles: string[];
  mockEffects: boolean;
}
export interface RegressionMockEffect {
  key: string;
  payload: string;
}
export interface RegressionFixtureOracle {
  expectedFiles: Record<string, string>;
  expectedOutput: string;
  expectedMockEffects: RegressionMockEffect[];
}
export interface RegressionFixture {
  id: string;
  actor: RegressionActorFixture;
  oracle: RegressionFixtureOracle;
}
export interface RegressionSuite {
  name: string;
  fixtures: RegressionFixture[];
}
export interface TaskRegressionSuiteSummary {
  id: string;
  name: string;
  digest: string;
  fixtureCount: number;
  createdAtMs: number;
}
export type TaskRegressionToolPolicy = 'files' | 'filesAndMockEffects';
export interface TaskRegressionConfiguration {
  agentId: string;
  configDigest: string;
  provider: string | null;
  model: string;
  system: string | null;
}
/** Supported deltas only; the daemon pins the selected canonical config. */
export interface TaskRegressionVariantSelection {
  agentId: string;
  configDigest: string;
  model?: string;
  provider?: string;
  system?: string;
  toolPolicy?: TaskRegressionToolPolicy;
}
export interface TaskRegressionCompareRequest {
  suiteId: string;
  suiteDigest: string;
  baseline: TaskRegressionVariantSelection;
  candidate: TaskRegressionVariantSelection;
  repetitions: number;
  /** Preserve on retries of the same owner action. */
  requestKey: string;
}
export interface TaskRegressionVariantPin {
  agentId: string;
  configDigest: string;
  effectiveConfigDigest: string;
  provider: string;
  model: string;
  toolPolicy: TaskRegressionToolPolicy;
}
export interface TaskRegressionTokenUsage {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
  cached_prompt_tokens: number;
  reasoning_tokens: number;
}
/** Missing provider usage is unknown, never a zero-token measurement. */
export interface TraceMetrics {
  modelCalls: number;
  toolCalls: number;
  toolErrors: number;
  runErrors: number;
  exactRepeatCalls: number;
  recoveredToolResults: number;
  resultBytes: number;
  finalOutputBytes: number;
  knownUsageCalls: number;
  unknownUsageCalls: number;
  knownUsage: TaskRegressionTokenUsage | null;
  elapsedMs: number;
}
export interface TaskRegressionArtifactObservationRef {
  reportSha256: string;
  observationLocator: string;
}
export type TaskRegressionVariant = 'baseline' | 'candidate';
export interface TaskRegressionTrial {
  fixtureId: string;
  repetition: number;
  variant: TaskRegressionVariant;
  observationDigest: string;
  observedFiles: Record<string, string>;
  observedMockEffects: RegressionMockEffect[];
  finalOutput: string | null;
  metrics: TraceMetrics;
  artifactRefs: TaskRegressionArtifactObservationRef[];
}
export interface TaskRegressionVariantSummary {
  trials: number;
  correct: number;
  metrics: TraceMetrics;
}
export interface TaskRegressionComparison {
  matrixComplete: boolean;
  baseline: TaskRegressionVariantSummary;
  candidate: TaskRegressionVariantSummary;
  trials: {
    fixtureId: string;
    repetition: number;
    variant: TaskRegressionVariant;
    failures: string[];
  }[];
}
/** Owner-only deterministic evidence; a report grants no approval or access. */
export interface TaskRegressionReport {
  id: string;
  status:
    | 'running'
    | 'completed'
    | 'inconclusive'
    | 'cancelled'
    | 'interrupted'
    | 'stale';
  suite: TaskRegressionSuiteSummary;
  baseline: TaskRegressionVariantPin;
  candidate: TaskRegressionVariantPin;
  repetitions: number;
  plannedTrials: number;
  completedTrials: number;
  createdAtMs: number;
  finishedAtMs: number | null;
  retiredAtMs: number | null;
  evidenceScope: 'selectedAdapterTasks' | 'runtimePlumbing';
  evaluatorDigest: string;
  toolImplementationDigest: string;
  hostSourceDigest: string;
  planDigest: string;
  comparison: TaskRegressionComparison | null;
  observations: TaskRegressionTrial[];
  usageComplete: boolean;
  limitations: string[];
  error: string | null;
}
type ReadOptions = { signal?: AbortSignal };

/** Thin owner transport. Constructing or reading this client never starts a run. */
export class TaskRegressionsClient {
  constructor(private readonly client: DaemonClient) {}

  async suites(
    options: ReadOptions = {},
  ): Promise<TaskRegressionSuiteSummary[]> {
    return (
      await this.client.requestJson<{ suites: TaskRegressionSuiteSummary[] }>(
        '/api/task-regression-suites',
        { ...options, cache: 'no-store' },
      )
    ).suites;
  }
  async define(suite: RegressionSuite): Promise<TaskRegressionSuiteSummary> {
    return (
      await this.client.requestJson<{ suite: TaskRegressionSuiteSummary }>(
        '/api/task-regression-suites',
        { method: 'POST', body: suite, cache: 'no-store' },
      )
    ).suite;
  }
  async configuration(
    agentId: string,
    options: ReadOptions = {},
  ): Promise<TaskRegressionConfiguration> {
    return (
      await this.client.requestJson<{
        configuration: TaskRegressionConfiguration;
      }>(`/api/task-regression-configs/${encodeURIComponent(agentId)}`, {
        ...options,
        cache: 'no-store',
      })
    ).configuration;
  }
  async compare(
    input: TaskRegressionCompareRequest,
  ): Promise<TaskRegressionReport> {
    return (
      await this.client.requestJson<{ report: TaskRegressionReport }>(
        '/api/task-regressions',
        { method: 'POST', body: input, cache: 'no-store' },
      )
    ).report;
  }
  async report(
    id: string,
    options: ReadOptions = {},
  ): Promise<TaskRegressionReport> {
    return (
      await this.client.requestJson<{ report: TaskRegressionReport }>(
        `/api/task-regressions/${encodeURIComponent(id)}`,
        { ...options, cache: 'no-store' },
      )
    ).report;
  }
  async cancel(id: string): Promise<TaskRegressionReport> {
    return (
      await this.client.requestJson<{ report: TaskRegressionReport }>(
        `/api/task-regressions/${encodeURIComponent(id)}/cancel`,
        { method: 'POST', body: {}, cache: 'no-store' },
      )
    ).report;
  }
}

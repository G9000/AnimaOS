import type { AgentToolInput } from './agents.js';
import type { DaemonPluginDescriptor } from './daemon-types.js';
import type { ModelChoice } from './provider-keys.js';

export type UUID = `${string}-${string}-${string}-${string}-${string}`;

export interface Attachment {
  type: 'file' | 'image' | 'url';
  name: string;
  data: string;
}

export interface Content {
  text: string;
  attachments?: Attachment[];
  metadata?: Record<string, unknown>;
}

export interface TaskResult<T = unknown> {
  status: 'success' | 'error';
  data?: T;
  error?: string;
  durationMs: number;
}

export interface TokenUsage {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
}

export type AgentStatus =
  | 'idle'
  | 'running'
  | 'completed'
  | 'failed'
  | 'terminated';

/** Finite limits shared by a turn, its helpers and reviewed recovery attempts. */
export interface ExecutionLimits {
  maxWallTimeMs?: number;
  maxInputTokens?: number;
  maxOutputTokens?: number;
  maxTotalTokens?: number;
  maxModelAttempts?: number;
  maxToolCalls?: number;
  maxRetries?: number;
  maxInflightOperations?: number;
}

/** A named owner-configured provider and model route for helpers. */
export interface DelegationRoute extends ModelChoice {
  id: string;
  fallbacks?: ModelChoice[];
}

/** Finite helper limits. Omitted fields use the daemon's bounded defaults. */
export interface DelegationPolicy {
  maxConcurrent?: number;
  maxStartsPerRun?: number;
  /** Only depth 1 is currently supported. */
  maxDepth?: 1;
  routes?: DelegationRoute[];
  /** Context forking is currently unsupported. */
  allowContextFork?: false;
  maxToolIterations?: number;
  timeoutMs?: number;
  maxOutputTokens?: number;
}

export interface AgentSettings {
  temperature?: number;
  maxTokens?: number;
  timeoutMs?: number;
  maxRetries?: number;
  maxToolIterations?: number;
  executionLimits?: ExecutionLimits;
  delegation?: DelegationPolicy;
  [key: string]: unknown;
}

/** Agent definition accepted by the daemon's `POST /api/agents`. */
export interface AgentConfig {
  name: string;
  model: string;
  bio?: string;
  lore?: string;
  knowledge?: string[];
  topics?: string[];
  adjectives?: string[];
  style?: string;
  provider?: string;
  system?: string;
  tools?: AgentToolInput[];
  plugins?: DaemonPluginDescriptor[];
  settings?: AgentSettings;
}

export interface AgentState {
  id: UUID;
  /** Display name mirrored from `config.name`. */
  name: string;
  status: AgentStatus;
  config: AgentConfig;
  createdAtMs: number;
  tokenUsage: TokenUsage;
}

import type { AgentToolInput } from './agents.js';
import type { DaemonPluginDescriptor } from './daemon-types.js';

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

export interface AgentSettings {
  temperature?: number;
  maxTokens?: number;
  timeoutMs?: number;
  maxRetries?: number;
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

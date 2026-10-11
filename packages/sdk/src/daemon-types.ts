import type { AgentStatus, Attachment, TokenUsage } from '@animaOS-SWARM/core';

export type AgentFileKind = 'soul' | 'anima' | 'agents' | 'claude' | 'cursor';
export type AgentFileState =
  | 'loaded'
  | 'truncated'
  | 'shadowed'
  | 'duplicate'
  | 'empty'
  | 'missing'
  | 'unreadable'
  | 'invalid_utf8'
  | 'linked'
  | 'non_regular'
  | 'oversized'
  | 'blocked'
  | 'budget_excluded'
  | 'timed_out'
  | 'cancelled';
export type AgentFileReasonCode =
  | 'read_failed'
  | 'invalid_scope'
  | 'file_too_large'
  | 'scan_limit'
  | 'threat_detected'
  | 'total_read_limit'
  | 'candidate_limit'
  | 'render_budget'
  | 'deadline'
  | 'cancelled';
export interface AgentFileManifestEntry {
  relativePath: string;
  kind: AgentFileKind;
  state: AgentFileState;
  rawByteCount: number;
  admittedCharCount: number;
  digest: string | null;
  truncated: boolean;
  threatIds: string[];
  reasonCode: AgentFileReasonCode | null;
}

/** Metadata only. Current files may differ from an active run's fixed snapshot. */
export interface AgentFilesPreview {
  mode: 'current_preview';
  availability: 'available' | 'unavailable_workspace' | 'stale_scope';
  capturedAtMs: number;
  snapshotDigest: string;
  identitySource: 'legacy' | 'workspace_soul' | 'profile_soul';
  projectSourceKind: Exclude<AgentFileKind, 'soul'> | null;
  estimatedTokens: number;
  entries: AgentFileManifestEntry[];
}

/** JSON returned by the daemon; distinct from in-process runtime objects. */
export interface DaemonContent {
  text: string;
  attachments: Attachment[] | null;
  metadata: Record<string, unknown> | null;
}

export interface DaemonTaskResult<T = DaemonContent> {
  status: 'success' | 'error';
  data: T | null;
  error: string | null;
  durationMs: number;
}

export interface DaemonToolDescriptor {
  name: string;
  description: string;
  parameters: Record<string, unknown>;
  examples: Array<{
    input: string;
    args: Record<string, unknown>;
    output: string;
  }> | null;
}

export interface DaemonPluginDescriptor {
  name: string;
  description: string;
}

export interface DaemonAgentSettings {
  temperature: number | null;
  maxTokens: number | null;
  timeoutMs: number | null;
  maxRetries: number | null;
  maxToolIterations: number | null;
  additional: Record<string, unknown>;
}

export interface DaemonAgentConfig {
  name: string;
  model: string;
  bio: string | null;
  lore: string | null;
  knowledge: string[] | null;
  topics: string[] | null;
  adjectives: string[] | null;
  style: string | null;
  provider: string | null;
  system: string | null;
  tools: DaemonToolDescriptor[] | null;
  plugins: DaemonPluginDescriptor[] | null;
  settings: DaemonAgentSettings | null;
}

export interface DaemonAgentState {
  id: string;
  name: string;
  status: AgentStatus;
  config: DaemonAgentConfig;
  createdAtMs: number;
  tokenUsage: TokenUsage;
}

export interface DaemonAgentMessage {
  id: string;
  agentId: string;
  roomId: string;
  content: DaemonContent;
  role: 'user' | 'assistant' | 'system' | 'tool';
  createdAtMs: number;
}

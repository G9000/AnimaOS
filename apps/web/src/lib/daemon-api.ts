// Typed client for the anima-daemon server (hosts/rust-daemon).
// The daemon owns a collection of durable agent snapshots. UI surfaces choose
// the agent they need instead of relying on GET /api/agents response order.
// All URLs are relative so the Vite dev proxy owns the origin
// (see vite.config.mts: '/api' -> UI_BACKEND_ORIGIN ?? http://localhost:8080).

import { PROVIDER_MODELS, type AgentDetail, type ChatMessage } from './types';
import {
  createDaemonClient,
  type ActiveHours,
  type AgencyGenerateRequest,
  type AgencyGenerateResponse,
  type ApprovalDecisionInput,
  type ApprovalListOptions,
  type ApprovalPolicy,
  type ApprovalRuleInput,
  type AutomationInput,
  type AutomationPatch,
  type AutomationTrigger,
  type HeartbeatInput,
  type MemoryFactOptions,
  type CoreMemoryBlockName,
  type MemoryExportOptions,
  type McpCreateInput,
  type McpUpdateInput,
  type McpVersionInput,
  type McpBearerCredentialInput,
  type McpReviewInput,
  type McpGrantInput,
  type BundleReviewInput,
  type BundleInstallInput,
  type BundleUpdateInput,
  type BundleEnabledInput,
  type BundleStateInput,
  type LogLevel,
  type LogsQuery,
  type MemoryPatch,
  type PricingOverride,
  type RelationshipEndpointKind,
  type WorkspaceBootstrapRequest,
  type AgentMemory,
  type AgentTasks,
  type AgentJobInput,
  type AgentJobRetryInput,
  type AgentJobReviewInput,
  type AddProviderKeyInput,
  type KeyStrategy,
  type ModelChoice,
  type ProviderKey,
  type ProviderKeys,
  type CreateMemoryInput,
  type CreateDiscordConnectorInput,
  type DiscordConnectorSettings,
  type ConnectDriveInput,
  type GoalInput,
  type GoalStatus,
  type LearningDecision,
  type LearningSkillDraftInput,
  type Session,
  type SessionListOptions,
  type SessionMessage,
  type SessionMessageOptions,
  type SessionMessageSearchOptions,
  type SessionUpdateInput,
  type SessionCompactInput,
  type SessionReviseInput,
  type SessionQueuedRunPatch,
  type SkillDraftApproval,
  type SkillInput,
  type StartRunInput,
  type UsageQuery,
  type UsageRecordsQuery,
  type WorkflowTemplateInput,
  type WorkflowTemplateRunInput,
  type SkillSupportingFile,
  type WorkflowInput,
  type WorkflowResumeInput,
} from '@animaos/sdk';
import { consoleSessionHeaders } from './console-session';

export type {
  AddProviderKeyInput,
  KeyStrategy,
  ModelChoice,
  ProviderKey,
  ProviderKeys,
  DiscordConnector,
  DiscordConnectorStatus,
  DiscordConnectorSettings,
  DiscordConversation,
  DiscordBotCheck,
  CreateDiscordConnectorInput,
  DriveConnector,
  DriveStatus,
  DriveConnectResult,
  ConnectDriveInput,
  McpConnection,
  McpToolDescriptor,
  ReviewedMcpTool,
  McpGrantOutcome,
  BundleSummary,
  BundleView,
  BundleReview,
  BundleComponentView,
  ComponentReview,
  CommunityManifest,
} from '@animaos/sdk';

/** Shared owner transport policy, for both fetch and multipart XHR: the
 *  console session's token half, when the console has one. */
export function daemonRequestHeaders(): Record<string, string> {
  return consoleSessionHeaders();
}

const setupClient = createDaemonClient({
  baseUrl: '',
  fetch: (url, init) => {
    const headers = daemonRequestHeaders();
    new Headers(init?.headers).forEach((value, name) => {
      headers[name] = value;
    });
    return globalThis.fetch(url, { ...init, headers });
  },
});

/** The Anima app's lifecycle (`/api/system`), with the owner transport. */
export const systemApi = setupClient.system;

/** All external-tool owner actions use the shared owner transport policy. */
export const mcpOwnerApi = {
  list: (agentId: string, options?: { signal?: AbortSignal }) =>
    setupClient.mcp.list(agentId, options),
  toolNames: async (agentId: string): Promise<string[]> => {
    const snapshot = await setupClient.agents.get(agentId);
    return snapshot.state.config.tools?.map((tool) => tool.name) ?? [];
  },
  create: (agentId: string, input: McpCreateInput) =>
    setupClient.mcp.create(agentId, input),
  update: (agentId: string, id: string, input: McpUpdateInput) =>
    setupClient.mcp.update(agentId, id, input),
  connect: (agentId: string, id: string, input: McpVersionInput) =>
    setupClient.mcp.connect(agentId, id, input),
  discover: (agentId: string, id: string, input: McpVersionInput) =>
    setupClient.mcp.discover(agentId, id, input),
  review: (agentId: string, id: string, input: McpReviewInput) =>
    setupClient.mcp.review(agentId, id, input),
  disconnect: (agentId: string, id: string, input: McpVersionInput) =>
    setupClient.mcp.disconnect(agentId, id, input),
  remove: (agentId: string, id: string, input: McpVersionInput) =>
    setupClient.mcp.remove(agentId, id, input),
  saveBearerCredential: (
    agentId: string,
    id: string,
    input: McpBearerCredentialInput,
  ) => setupClient.mcp.saveBearerCredential(agentId, id, input),
  clearBearerCredential: (
    agentId: string,
    id: string,
    input: McpVersionInput,
  ) => setupClient.mcp.clearBearerCredential(agentId, id, input),
  setGrant: (
    agentId: string,
    id: string,
    alias: string,
    input: McpGrantInput,
  ) => setupClient.mcp.setGrant(agentId, id, alias, input),
};

/** Bundle configuration shares owner transport and existing canonical grants. */
export const communityBundlesOwnerApi = {
  list: (agentId: string, options?: { signal?: AbortSignal }) =>
    setupClient.communityBundles.list(agentId, options),
  get: (agentId: string, id: string, options?: { signal?: AbortSignal }) =>
    setupClient.communityBundles.get(agentId, id, options),
  review: (agentId: string, input: BundleReviewInput) =>
    setupClient.communityBundles.review(agentId, input),
  install: (agentId: string, input: BundleInstallInput) =>
    setupClient.communityBundles.install(agentId, input),
  update: (agentId: string, id: string, input: BundleUpdateInput) =>
    setupClient.communityBundles.update(agentId, id, input),
  setEnabled: (agentId: string, id: string, input: BundleEnabledInput) =>
    setupClient.communityBundles.setEnabled(agentId, id, input),
  remove: (agentId: string, id: string, input: BundleStateInput) =>
    setupClient.communityBundles.remove(agentId, id, input),
  toolNames: (agentId: string) => mcpOwnerApi.toolNames(agentId),
};

export interface DaemonProvider {
  id: string;
  label: string;
  requiresKey: boolean;
  /** true when the daemon has a usable API key / local runtime for this provider */
  configured: boolean;
  apiKeyEnvs: string[];
}

/**
 * The built-in mock providers: fine as an agent's own model, but the daemon
 * refuses them as a fallback or the side-task model.
 */
export function isMockProvider(id: string): boolean {
  const normalized = id.trim().toLowerCase();
  return normalized === 'deterministic' || normalized === 'test';
}

interface DaemonTokenUsage {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
}

interface DaemonMessage {
  id: string;
  agentId: string;
  roomId: string;
  content: { text: string; metadata?: Record<string, unknown> | null };
  role: string;
  createdAtMs: number;
}

export interface DaemonToolExample {
  input: string;
  args: Record<string, unknown>;
  output: string;
}

export interface DaemonToolDescriptor {
  name: string;
  description: string;
  parameters: Record<string, unknown>;
  examples?: DaemonToolExample[] | null;
}

export interface DaemonSnapshot {
  state: {
    id: string;
    name: string;
    status: string;
    config: {
      name: string;
      model: string;
      provider?: string | null;
      system?: string | null;
      bio?: string | null;
      adjectives?: string[] | null;
      style?: string | null;
      tools?: DaemonToolDescriptor[] | null;
      /** Omitted by the daemon when there are none. */
      fallbacks?: ModelChoice[] | null;
      settings?: { additional?: Record<string, unknown> } | null;
    };
    createdAtMs: number;
    tokenUsage: DaemonTokenUsage;
  };
  messageCount: number;
  messages: DaemonMessage[];
  eventCount: number;
}

export interface AgentUpdateInput {
  name?: string;
  model?: string;
  provider?: string;
  system?: string;
  tools?: string[];
  /** At most three, none equal to the primary; `[]` clears them. */
  fallbacks?: ModelChoice[];
}

export type TelegramConnectorStatus =
  | 'ready'
  | 'pairing'
  | 'credentialRequired'
  | 'error'
  | 'degraded'
  | 'reconciling'
  | 'pollConflict';

export interface TelegramChat {
  id: string;
  kind: 'private' | 'group' | 'supergroup' | 'channel';
  title: string | null;
  username: string | null;
}

export interface TelegramConnector {
  id: string;
  agentId: string;
  roomId: string;
  type: 'telegram';
  bot: { id: string; username: string | null; displayName: string | null };
  approvedChat: TelegramChat | null;
  pendingPairing: { chat: TelegramChat; requestedAtMs: number } | null;
  status: TelegramConnectorStatus;
  enabled: boolean;
  createdAtMs: number;
  updatedAtMs: number;
}

export interface ConnectorMessage {
  id: string;
  agentId: string;
  roomId: string;
  content: { text: string; metadata?: Record<string, unknown> | null };
  role: 'user' | 'assistant' | 'system' | 'tool';
  createdAtMs: number;
}

export type ScheduleTrigger = AutomationTrigger;
export type ScheduleTarget =
  | { type: 'workspace' }
  | { type: 'connector'; connectorId: string };
export interface ScheduleOutcome {
  status: 'silent' | 'spoke' | 'error' | 'stopped';
  occurredAtMs: number;
  errorCode: string | null;
}
export interface DaemonSchedule {
  id: string;
  importIdempotencyKey: string | null;
  agentId: string;
  prompt: string;
  trigger: ScheduleTrigger;
  enabled: boolean;
  target: ScheduleTarget;
  nextDueAtMs: number;
  lastFiredAtMs: number | null;
  lastOutcome: ScheduleOutcome | null;
  createdAtMs: number;
  updatedAtMs: number;
}

export interface ScheduleCreateInput {
  prompt: string;
  trigger: ScheduleTrigger;
  target: ScheduleTarget;
  enabled?: boolean;
  importIdempotencyKey?: string;
}

export interface LegacyScheduleInput {
  id: string;
  prompt: string;
  intervalSecs: number;
  createdAtMs: number;
  lastRunAtMs?: number;
  target?: ScheduleTarget;
}

/** Model suggestions keyed by daemon provider id. */
export const MODEL_SUGGESTIONS: Record<string, string[]> = {
  ...PROVIDER_MODELS,
  deterministic: ['deterministic'],
};

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`/api${path}`, {
    ...init,
    headers: {
      ...daemonRequestHeaders(),
      ...(init?.body ? { 'content-type': 'application/json' } : {}),
      ...init?.headers,
    },
  });
  if (!response.ok) {
    let message = `daemon request failed (${response.status})`;
    try {
      const body = (await response.json()) as {
        error?: string;
        message?: string;
      };
      message = body.error ?? body.message ?? message;
    } catch {
      /* keep default message */
    }
    const error = new Error(message) as Error & { status?: number };
    error.status = response.status;
    throw error;
  }
  if (response.status === 204) return undefined as T;
  return (await response.json()) as T;
}

/** A key the owner may hand to coding apps this agent starts (fixed set;
 *  the value never leaves the daemon). */
export interface CodingKey {
  name: string;
  label: string;
  /** The daemon's secret store holds a value for it. */
  saved: boolean;
  /** Handed to coding apps this agent starts. */
  enabled: boolean;
}

/** How much an agent asks before it acts (spec 2026-10-19). `custom`
 *  is never sent: it shows when the class policy matches neither preset. */
export type AutonomyMode = 'free' | 'strict' | 'custom';

/** The most deny patterns, and the longest one, the daemon keeps. */
export const MAX_DENY_PATTERNS = 50;
export const MAX_DENY_PATTERN_CHARS = 200;
/** The longest smart-check guidance the daemon keeps. */
export const MAX_SMART_POLICY_CHARS = 1000;

export interface AgentAutonomy {
  mode: AutonomyMode;
  /** The agent's own model judges risky commands before the owner is asked. */
  smartCheck: boolean;
  /** The owner's always-blocked command globs (`*`, `?`). */
  denyPatterns: string[];
  /** The owner's written guidance for the smart check; empty for none. */
  smartPolicy: string;
  policy: ApprovalPolicy;
}

export interface AutonomyPatch {
  mode?: Exclude<AutonomyMode, 'custom'>;
  smartCheck?: boolean;
  denyPatterns?: string[];
  smartPolicy?: string;
}

export type ForeignSource = 'claude-code' | 'codex';

/** A Claude Code or Codex conversation on this computer. `handle` stands
 *  for its file; the daemon never sends paths. */
export interface ForeignSession {
  handle: string;
  source: ForeignSource;
  title: string;
  cwd: string | null;
  /** Epoch milliseconds. */
  modifiedAt: number;
  sizeBytes: number;
  foreignSessionId: string | null;
}

export interface ForeignSessionPreview {
  title: string;
  cwd: string | null;
  source: ForeignSource;
  messageCount: number;
  messages: { role: 'user' | 'assistant'; text: string; at: number | null }[];
}

export interface ForeignImport {
  sessionId: string;
  title: string;
  messageCount: number;
  alreadyImported: boolean;
}

/** A coding CLI the daemon looked for on this computer. */
export interface CodingApp {
  id: string;
  name: string;
  found: boolean;
  version: string | null;
  /** Set when it is installed but cannot start. */
  problem: string | null;
  bundleId: string;
  /** Its know-how bundle's manifest, installed through the community
   *  bundle review. */
  manifestText: string;
}

/** A know-how bundle the daemon ships (the generic coding handoff). */
export interface CodingKnowHow {
  bundleId: string;
  manifestText: string;
}

/** Error.message prefix returned by the daemon when no generative provider is available. */
export const PROFILE_GENERATION_UNAVAILABLE = 'PROFILE_GENERATION_UNAVAILABLE';

export interface WorkspaceConfigInput {
  rootPath: string;
  companyName: string;
  mission: string;
  values: string[];
}

export interface DaemonWorkspaceConfig extends WorkspaceConfigInput {
  hasAvatar: boolean;
}

export interface DaemonWorkspaceState {
  configured: boolean;
  workspace: DaemonWorkspaceConfig | null;
  defaultRoot: string;
  /** Present only on validate-only responses: does the folder already exist? */
  rootPathExists?: boolean;
}

/** Validate-only response: the daemon always sets rootPathExists here, never elsewhere. */
export type DaemonWorkspaceValidation = DaemonWorkspaceState & {
  rootPathExists: boolean;
};

export const workspaceAvatarUrl = (revision: number) =>
  `/api/workspace/avatar?v=${revision}`;

export interface GenerateProfileInput {
  presetId: string;
  intent: string;
  provider: string;
  model: string;
  workspace: { companyName: string; mission: string; values: string[] };
}

export interface AgentProfile {
  bio: string;
  adjectives: string[];
  style: string;
  system: string;
}

// UI drafts tolerate partial previews in tests; real generation uses the SDK wire contract.
export type GeneratedAgency = Pick<AgencyGenerateResponse, 'name'> &
  Partial<Omit<AgencyGenerateResponse, 'name' | 'agents'>> & {
    agents: Array<
      Pick<AgencyGenerateResponse['agents'][number], 'name' | 'role'> &
        Partial<
          Omit<AgencyGenerateResponse['agents'][number], 'name' | 'role'>
        > & { provider?: string | null }
    >;
  };

export interface BootstrapWorkspaceInput extends WorkspaceBootstrapRequest {
  workspace: WorkspaceConfigInput;
  workers?: BootstrapWorkspaceInput['agent'][];
  agent: {
    name: string;
    presetId: string;
    bio: string;
    adjectives?: string[];
    style?: string;
    system: string;
    provider?: string;
    model: string;
    tools: string[];
  };
}

export interface WorkspaceInspectAgentPreview {
  name: string;
  bio?: string;
  provider: string;
  model: string;
}

export interface WorkspaceInspectFound {
  found: true;
  companyName: string;
  /** Omitted when the yaml lacks a mission (description fallback may be blank). */
  mission?: string;
  /** Omitted when the yaml lacks values. */
  values?: string[];
  orchestrator: WorkspaceInspectAgentPreview;
  workers: WorkspaceInspectAgentPreview[];
  providerAvailable: boolean;
}

export type WorkspaceInspectResponse = { found: false } | WorkspaceInspectFound;

export interface WorkspaceResumeResponse {
  workspace: DaemonWorkspaceConfig;
  orchestrator: DaemonSnapshot;
  workers: DaemonSnapshot[];
  skipped: string[];
}

export const daemon = {
  runs: setupClient.runs,
  interactions: setupClient.interactions,
  voice: setupClient.voice,
  speech: setupClient.speech,
  browserScopes: setupClient.browserScopes,
  agentFilesPreview: (id: string, options: { signal?: AbortSignal } = {}) =>
    setupClient.agentFiles.preview(id, options),
  capabilities: () => setupClient.capabilities(),
  listWorkspaceFiles: () => setupClient.workspace.listFiles(),
  readWorkspaceFile: (path: string) => setupClient.workspace.readFile(path),
  downloadWorkspaceFile: (path: string, signal?: AbortSignal): Promise<Blob> =>
    setupClient.workspace.downloadFile(path, { signal }),
  agentTasks: (id: string) =>
    request<AgentTasks>(`/agents/${encodeURIComponent(id)}/tasks`),
  agentJobs: (id: string, options?: { signal?: AbortSignal }) =>
    setupClient.agents.jobs(id, options),
  createAgentJob: (id: string, input: AgentJobInput) =>
    setupClient.agents.createJob(id, input),
  listWorkflows: (agentId: string, options: { signal?: AbortSignal } = {}) =>
    setupClient.workflows.list(agentId, options),
  getWorkflow: (
    agentId: string,
    id: string,
    options: { signal?: AbortSignal } = {},
  ) => setupClient.workflows.get(agentId, id, options),
  createWorkflow: (agentId: string, input: WorkflowInput) =>
    setupClient.workflows.create(agentId, input),
  resumeWorkflow: (agentId: string, id: string, input: WorkflowResumeInput) =>
    setupClient.workflows.resume(agentId, id, input),
  cancelWorkflow: (agentId: string, id: string, input: { revision: number }) =>
    setupClient.workflows.cancel(agentId, id, input),
  goals: (options: { signal?: AbortSignal } = {}) =>
    setupClient.goals.list(options),
  createGoal: (input: GoalInput) => setupClient.goals.create(input),
  setGoalStatus: (
    id: string,
    input: { revision: number; status: GoalStatus },
  ) => setupClient.goals.setStatus(id, input),
  learningPolicy: (agentId: string, options: { signal?: AbortSignal } = {}) =>
    setupClient.learning.policy(agentId, options),
  setLearningPolicy: (agentId: string, input: { autoEnabled: boolean }) =>
    setupClient.learning.setPolicy(agentId, input),
  requestLearningReview: (agentId: string, runId: string) =>
    setupClient.learning.requestReview(agentId, runId),
  listLearningReviews: (
    agentId: string,
    options: { signal?: AbortSignal } = {},
  ) => setupClient.learning.listReviews(agentId, options),
  getLearningReview: (
    agentId: string,
    reviewId: string,
    options: { signal?: AbortSignal } = {},
  ) => setupClient.learning.getReview(agentId, reviewId, options),
  cancelLearningReview: (agentId: string, reviewId: string) =>
    setupClient.learning.cancelReview(agentId, reviewId),
  undoLearningLesson: (
    agentId: string,
    reviewId: string,
    proposalId: string,
    input: { expectedVersion: number },
  ) => setupClient.learning.undoLesson(agentId, reviewId, proposalId, input),
  decideLearningProposal: (
    agentId: string,
    reviewId: string,
    proposalId: string,
    input: { decision: LearningDecision; expectedVersion: number },
  ) =>
    setupClient.learning.decideProposal(agentId, reviewId, proposalId, input),
  createLearningSkillDraft: (
    agentId: string,
    reviewId: string,
    proposalId: string,
    input: LearningSkillDraftInput,
  ) =>
    setupClient.learning.createSkillDraft(agentId, reviewId, proposalId, input),
  goalJobs: (id: string, options: { signal?: AbortSignal } = {}) =>
    setupClient.goals.jobs(id, options),
  listSessions: (agentId: string, options: SessionListOptions = {}) =>
    setupClient.sessions.list(agentId, options),
  getSession: (agentId: string, sessionId: string) =>
    setupClient.sessions.get(agentId, sessionId),
  ensureMainSession: (agentId: string): Promise<Session> =>
    setupClient.sessions.main(agentId),
  createSession: (
    agentId: string,
    input: { title?: string } = {},
  ): Promise<Session> => setupClient.sessions.create(agentId, input),
  downloadAttachment: (
    agentId: string,
    attachmentId: string,
    options: { signal?: AbortSignal } = {},
  ) => setupClient.attachments.download(agentId, attachmentId, options),
  updateSession: (
    agentId: string,
    sessionId: string,
    patch: SessionUpdateInput,
  ) => setupClient.sessions.update(agentId, sessionId, patch),
  clearSession: async (agentId: string, sessionId: string) =>
    (
      await request<{ session: Session }>(
        `/agents/${encodeURIComponent(agentId)}/sessions/${encodeURIComponent(sessionId)}/clear`,
        { method: 'POST' },
      )
    ).session,
  deleteSession: (agentId: string, sessionId: string) =>
    setupClient.sessions.remove(agentId, sessionId),
  sessionMessages: (
    agentId: string,
    sessionId: string,
    options: SessionMessageOptions = {},
  ) => setupClient.sessions.messages(agentId, sessionId, options),
  searchSessionMessages: (
    agentId: string,
    options: SessionMessageSearchOptions,
  ) => setupClient.sessions.searchMessages(agentId, options),
  exportSession: (agentId: string, sessionId: string) =>
    setupClient.sessions.exportMarkdown(agentId, sessionId),
  reviseSession: (
    agentId: string,
    sessionId: string,
    input: SessionReviseInput,
  ) => setupClient.sessions.revise(agentId, sessionId, input),
  updateQueuedRun: (
    agentId: string,
    sessionId: string,
    runId: string,
    patch: SessionQueuedRunPatch,
  ) => setupClient.sessions.updateQueuedRun(agentId, sessionId, runId, patch),
  /** Accepts a message into a session (spec §4.2). */
  startRun: (
    agentId: string,
    sessionId: string,
    input: StartRunInput,
    idempotencyKey: string,
  ) => setupClient.runs.start(agentId, sessionId, input, { idempotencyKey }),
  stopRun: (agentId: string, runId: string) =>
    setupClient.runs.stop(agentId, runId),
  sessionRuns: (
    agentId: string,
    sessionId: string,
    options: { limit?: number; signal?: AbortSignal } = {},
  ) => setupClient.runs.listForSession(agentId, sessionId, options),
  compactSession: (
    agentId: string,
    sessionId: string,
    input?: SessionCompactInput,
  ) => setupClient.sessions.compact(agentId, sessionId, input),
  /** The companion's live event stream (spec §6). */
  agentEvents: (agentId: string, options: { signal?: AbortSignal } = {}) =>
    setupClient.events.stream(agentId, options),
  /** The owner's decision on a pending approval (spec §7.3). */
  decideApproval: (approvalId: string, input: ApprovalDecisionInput) =>
    setupClient.approvals.decide(approvalId, input),
  listApprovals: (options: ApprovalListOptions) =>
    setupClient.approvals.list(options),
  approvalPolicy: (agentId: string, options: { signal?: AbortSignal } = {}) =>
    setupClient.approvals.policy(agentId, options),
  setApprovalPolicy: (agentId: string, policy: ApprovalPolicy) =>
    setupClient.approvals.setPolicy(agentId, policy),
  approvalRules: (agentId: string, options: { signal?: AbortSignal } = {}) =>
    setupClient.approvals.rules(agentId, options),
  addApprovalRule: (agentId: string, input: ApprovalRuleInput) =>
    setupClient.approvals.addRule(agentId, input),
  removeApprovalRule: (agentId: string, ruleId: string) =>
    setupClient.approvals.removeRule(agentId, ruleId),
  /** Memory (spec §10). Reads take no signal: the SDK memory calls do not;
   *  the hook ignores stale answers by sequence number. */
  memoryReadiness: () => setupClient.memories.readiness(),
  recentMemories: (agentId: string, limit: number) =>
    setupClient.memories.recent({ agentId, limit }),
  searchMemories: (query: string, agentId: string, limit: number) =>
    setupClient.memories.search(query, { agentId, limit }),
  traceMemory: (id: string) => setupClient.memories.trace(id),
  updateMemory: (id: string, patch: MemoryPatch) =>
    setupClient.memories.update(id, patch),
  deleteMemory: (id: string, expectedContent?: string) =>
    setupClient.memories.delete(id, expectedContent),
  saveMemory: (input: CreateMemoryInput) => setupClient.memories.create(input),
  listFacts: (options: MemoryFactOptions) =>
    setupClient.memories.facts(options),
  coreMemory: (agentId: string, options: { signal?: AbortSignal } = {}) =>
    setupClient.memories.coreMemory(agentId, options),
  addCoreEntry: (agentId: string, block: CoreMemoryBlockName, text: string) =>
    setupClient.memories.addCoreEntry(agentId, block, text),
  updateCoreEntry: (
    agentId: string,
    block: CoreMemoryBlockName,
    entryId: string,
    text: string,
  ) => setupClient.memories.updateCoreEntry(agentId, block, entryId, text),
  removeCoreEntry: (
    agentId: string,
    block: CoreMemoryBlockName,
    entryId: string,
  ) => setupClient.memories.removeCoreEntry(agentId, block, entryId),
  /** The owner's memory as a file: JSON or Markdown, one agent or all. */
  exportMemories: (options: MemoryExportOptions = {}) =>
    setupClient.memories.exportMemories(options),
  replaceFact: (id: string, value: string) =>
    setupClient.memories.replaceFact(id, value),
  deleteFact: (id: string) => setupClient.memories.deleteFact(id),
  listMemoryEntities: (limit: number) =>
    setupClient.memories.entities({ limit }),
  listMemoryRelationships: (agentId: string, limit: number) =>
    setupClient.memories.relationships({ agentId, limit }),
  deleteMemoryEntity: (kind: RelationshipEndpointKind, id: string) =>
    setupClient.memories.deleteEntity(kind, id),

  listWorkflowTemplates: (
    agentId: string,
    options: { signal?: AbortSignal } = {},
  ) => setupClient.workflowTemplates.list(agentId, options),
  getWorkflowTemplate: (
    agentId: string,
    slug: string,
    options: { signal?: AbortSignal } = {},
  ) => setupClient.workflowTemplates.get(agentId, slug, options),
  saveWorkflowTemplate: (agentId: string, input: WorkflowTemplateInput) =>
    setupClient.workflowTemplates.save(agentId, input),
  deleteWorkflowTemplate: (
    agentId: string,
    slug: string,
    input: { revision: number },
  ) => setupClient.workflowTemplates.delete(agentId, slug, input),
  runWorkflowTemplate: (
    agentId: string,
    slug: string,
    input: WorkflowTemplateRunInput,
  ) => setupClient.workflowTemplates.run(agentId, slug, input),
  toolResults: setupClient.toolResults,
  /** Skills (spec §8.4); the daemon rescans the folder on these reads. */
  listSkills: (options: { signal?: AbortSignal } = {}) =>
    setupClient.skills.list(options),
  skill: (slug: string, options: { signal?: AbortSignal } = {}) =>
    setupClient.skills.get(slug, options),
  saveSkill: (slug: string, input: SkillInput) =>
    setupClient.skills.save(slug, input),
  setSkillEnabled: (slug: string, enabled: boolean) =>
    setupClient.skills.setEnabled(slug, enabled),
  deleteSkill: (slug: string) => setupClient.skills.remove(slug),
  approveSkill: (slug: string, hash: string) =>
    setupClient.skills.approve(slug, hash),
  listSkillDrafts: (options: {
    status: 'pending' | 'decided';
    signal?: AbortSignal;
  }) => setupClient.skills.drafts(options),
  approveSkillDraft: (id: string, approval: SkillDraftApproval = {}) =>
    setupClient.skills.approveDraft(id, approval),
  rejectSkillDraft: (id: string) => setupClient.skills.rejectDraft(id),
  importSkill: (file: File, files?: SkillSupportingFile[]) =>
    setupClient.skills.importFile(file, { filename: file.name, files }),
  /** Usage (spec §11, §15.4). */
  usageSummary: (query: UsageQuery) => setupClient.usage.summary(query),
  usageRecords: (query: UsageRecordsQuery) => setupClient.usage.records(query),
  exportUsageCsv: (query: { from?: number; to?: number; agentId?: string }) =>
    setupClient.usage.exportCsv(query),
  usagePricing: () => setupClient.usage.pricing(),
  setUsagePricing: (overrides: PricingOverride[]) =>
    setupClient.usage.setPricing(overrides),
  /** Logs and health (spec §11.2, §11.3, §15.4). */
  logs: (query: LogsQuery) => setupClient.logs.list(query),
  logStream: (options: {
    level?: LogLevel;
    q?: string;
    after?: number;
    signal?: AbortSignal;
  }) => setupClient.logs.stream(options),
  status: () => setupClient.status.get(),
  /** Automations (spec §9.2, §15.4). */
  listAutomations: (agentId: string, options: { signal?: AbortSignal } = {}) =>
    setupClient.automations.list(agentId, options),
  createAutomation: (agentId: string, input: AutomationInput) =>
    setupClient.automations.create(agentId, input),
  createHeartbeat: (agentId: string, input: HeartbeatInput) =>
    setupClient.automations.createHeartbeat(agentId, input),
  updateAutomation: (agentId: string, id: string, patch: AutomationPatch) =>
    setupClient.automations.update(agentId, id, patch),
  deleteAutomation: (agentId: string, id: string) =>
    setupClient.automations.remove(agentId, id),
  runAutomationNow: (agentId: string, id: string) =>
    setupClient.automations.runNow(agentId, id),
  /** Marks a one-time reminder done. */
  completeAutomation: (agentId: string, id: string) =>
    setupClient.automations.complete(agentId, id),
  /** Re-arms a fired one-time reminder `minutes` from now. */
  snoozeAutomation: (agentId: string, id: string, minutes: number) =>
    setupClient.automations.snooze(agentId, id, minutes),
  automationHistory: (
    agentId: string,
    id: string,
    options: { limit?: number; signal?: AbortSignal } = {},
  ) => setupClient.automations.history(agentId, id, options),
  previewAutomation: (
    input: { trigger: AutomationTrigger; activeHours?: ActiveHours },
    options: { signal?: AbortSignal } = {},
  ) => setupClient.automations.preview(input, options),
  listAgentSummaries: () => setupClient.agents.listSummaries(),
  cancelAgentJob: (id: string, jobId: string, input: { revision: number }) =>
    setupClient.agents.cancelJob(id, jobId, input),
  retryAgentJob: (id: string, jobId: string, input: AgentJobRetryInput) =>
    setupClient.agents.retryJob(id, jobId, input),
  approveAgentJob: (id: string, jobId: string, input: { revision: number }) =>
    setupClient.agents.approveJob(id, jobId, input),
  reviewAgentJob: (id: string, jobId: string, input: AgentJobReviewInput) =>
    setupClient.agents.reviewJob(id, jobId, input),
  updateAgentTasks: (id: string, input: AgentTasks) =>
    request<AgentTasks>(`/agents/${encodeURIComponent(id)}/tasks`, {
      method: 'PUT',
      body: JSON.stringify(input),
    }),
  health: () => request<{ status: string }>('/health'),

  /** Coding apps (app and ACP sessions the agent drives). */
  stopAppSession: (agentId: string, sessionId: string) =>
    request<{ stopped: boolean; treeGone: boolean }>(
      `/agents/${encodeURIComponent(agentId)}/app-sessions/${encodeURIComponent(sessionId)}/stop`,
      { method: 'POST' },
    ),
  cancelAcpTurn: (agentId: string, sessionId: string) =>
    request<{ cancelled: boolean }>(
      `/agents/${encodeURIComponent(agentId)}/acp-sessions/${encodeURIComponent(sessionId)}/cancel`,
      { method: 'POST' },
    ),
  stopAcpSession: (agentId: string, sessionId: string) =>
    request<{ stopped: boolean; treeGone: boolean }>(
      `/agents/${encodeURIComponent(agentId)}/acp-sessions/${encodeURIComponent(sessionId)}/stop`,
      { method: 'POST' },
    ),
  /** How much the agent asks; owner-only. A helper's answer is 409. */
  autonomy: (agentId: string) =>
    request<AgentAutonomy>(`/agents/${encodeURIComponent(agentId)}/autonomy`),
  setAutonomy: (agentId: string, patch: AutonomyPatch) =>
    request<AgentAutonomy>(`/agents/${encodeURIComponent(agentId)}/autonomy`, {
      method: 'PUT',
      body: JSON.stringify(patch),
    }),
  codingKeys: (agentId: string) =>
    request<{ keys: CodingKey[] }>(
      `/agents/${encodeURIComponent(agentId)}/coding-keys`,
    ),
  setCodingKeys: (agentId: string, enabled: string[]) =>
    request<{ keys: CodingKey[] }>(
      `/agents/${encodeURIComponent(agentId)}/coding-keys`,
      { method: 'PUT', body: JSON.stringify({ enabled }) },
    ),
  foreignSessions: (source: ForeignSource, cursor?: string | null) =>
    request<{ sessions: ForeignSession[]; nextCursor: string | null }>(
      `/foreign-sessions?source=${encodeURIComponent(source)}${
        cursor ? `&cursor=${encodeURIComponent(cursor)}` : ''
      }`,
    ),
  foreignSessionPreview: (handle: string) =>
    request<ForeignSessionPreview>(
      `/foreign-sessions/${encodeURIComponent(handle)}/preview`,
    ),
  importForeignSession: (agentId: string, handle: string) =>
    request<ForeignImport>(
      `/agents/${encodeURIComponent(agentId)}/foreign-sessions/${encodeURIComponent(handle)}/import`,
      { method: 'POST' },
    ),
  codingApps: () =>
    request<{ apps: CodingApp[]; handoff: CodingKnowHow | null }>(
      '/coding-apps',
    ),

  listProviders: () => request<{ providers: DaemonProvider[] }>('/providers'),

  /** The key pool per provider; keys only ever come back masked. */
  providerKeys: () => request<{ providers: ProviderKeys[] }>('/provider-keys'),
  /** 201 with the masked key; 400 and 409 carry the daemon's reason. */
  addProviderKey: (provider: string, input: AddProviderKeyInput) =>
    request<ProviderKey>(`/provider-keys/${encodeURIComponent(provider)}`, {
      method: 'POST',
      body: JSON.stringify(input),
    }),
  removeProviderKey: (provider: string, keyId: string) =>
    request<{ deleted: boolean }>(
      `/provider-keys/${encodeURIComponent(provider)}/${encodeURIComponent(keyId)}`,
      { method: 'DELETE' },
    ),
  /** "Try now": ends a resting key's rest, the env key included. */
  wakeProviderKey: (provider: string, keyId: string) =>
    request<ProviderKey>(
      `/provider-keys/${encodeURIComponent(provider)}/${encodeURIComponent(keyId)}/wake`,
      { method: 'POST' },
    ),
  setKeyStrategy: (provider: string, strategy: KeyStrategy) =>
    request<ProviderKeys>(
      `/provider-keys/${encodeURIComponent(provider)}/strategy`,
      { method: 'PUT', body: JSON.stringify({ strategy }) },
    ),
  /** The model for small background tasks, or null for each agent's own. */
  sideModel: () => request<ModelChoice | null>('/settings/side-model'),
  setSideModel: (choice: ModelChoice | null) =>
    request<ModelChoice | null>('/settings/side-model', {
      method: 'PUT',
      body: JSON.stringify(choice),
    }),

  listAgents: () => request<{ agents: DaemonSnapshot[] }>('/agents'),

  getAgent: (id: string) => request<{ agent: DaemonSnapshot }>(`/agents/${id}`),

  /** name, model, and tool names are required by the daemon. */
  createAgent: (input: {
    name: string;
    model: string;
    tools: string[];
    provider?: string;
    system?: string;
    settings?: { additional?: { workspaceRole?: 'lead' } };
  }) =>
    request<{ agent: DaemonSnapshot }>('/agents', {
      method: 'POST',
      body: JSON.stringify(input),
    }),

  deleteAgent: (id: string) =>
    request<{ deleted: boolean }>(`/agents/${id}`, { method: 'DELETE' }),

  /**
   * Partial config update; the conversation is kept. Empty string for
   * provider/system clears the field back to the daemon default.
   */
  updateAgent: (id: string, patch: AgentUpdateInput) =>
    request<{ agent: DaemonSnapshot }>(`/agents/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(patch),
    }),

  recentAgentMemories: (id: string) =>
    request<{ memories: AgentMemory[] }>(
      `/agents/${encodeURIComponent(id)}/memories/recent?limit=100`,
    ),
  setAgentAvatar: (id: string, file: File) =>
    request<void>(`/agents/${encodeURIComponent(id)}/avatar`, {
      method: 'PUT',
      body: file,
      headers: { 'content-type': file.type },
    }),
  removeAgentAvatar: (id: string) =>
    request<void>(`/agents/${encodeURIComponent(id)}/avatar`, {
      method: 'DELETE',
    }),

  listConnectors: (agentId: string) =>
    request<{ connectors: TelegramConnector[] }>(
      `/agents/${encodeURIComponent(agentId)}/connectors`,
    ),
  driveStatus: (agentId: string) => setupClient.connectors.driveStatus(agentId),
  connectDrive: (agentId: string, input: ConnectDriveInput) =>
    setupClient.connectors.connectDrive(agentId, input),
  disconnectDrive: (agentId: string, connectorId: string) =>
    setupClient.connectors.disconnectDrive(agentId, connectorId),
  discordConnectors: (agentId: string) =>
    setupClient.connectors.discordConnectors(agentId),
  checkDiscordBot: (agentId: string, botToken: string) =>
    setupClient.connectors.checkDiscordBot(agentId, botToken),
  configureDiscordConnector: (
    agentId: string,
    connectorId: string,
    settings: DiscordConnectorSettings,
    approvedUserIds: string[],
  ) =>
    setupClient.connectors.configureDiscordConnector(
      agentId,
      connectorId,
      settings,
      approvedUserIds,
    ),
  createDiscordConnector: (
    agentId: string,
    input: CreateDiscordConnectorInput,
  ) => setupClient.connectors.createDiscordConnector(agentId, input),
  activateDiscordConnector: (agentId: string, connectorId: string) =>
    setupClient.connectors.activateDiscordConnector(agentId, connectorId),
  replaceDiscordCredential: (
    agentId: string,
    connectorId: string,
    botToken: string,
  ) =>
    setupClient.connectors.replaceDiscordCredential(
      agentId,
      connectorId,
      botToken,
    ),
  setDiscordEnabled: (agentId: string, connectorId: string, enabled: boolean) =>
    setupClient.connectors.setDiscordEnabled(agentId, connectorId, enabled),
  restartDiscordConnector: (agentId: string, connectorId: string) =>
    setupClient.connectors.restartDiscordConnector(agentId, connectorId),
  retryDiscordDelivery: (agentId: string, connectorId: string) =>
    setupClient.connectors.retryDiscordDelivery(agentId, connectorId),
  suppressDiscordUnknownDelivery: (agentId: string, connectorId: string) =>
    setupClient.connectors.suppressDiscordUnknownDelivery(agentId, connectorId),
  deleteDiscordConnector: (agentId: string, connectorId: string) =>
    setupClient.connectors.deleteDiscordConnector(agentId, connectorId),
  createTelegramConnector: (agentId: string, botToken: string) =>
    request<{ connector: TelegramConnector }>(
      `/agents/${encodeURIComponent(agentId)}/connectors/telegram`,
      { method: 'POST', body: JSON.stringify({ botToken }) },
    ),
  replaceTelegramCredential: (
    agentId: string,
    connectorId: string,
    botToken: string,
  ) =>
    request<{ connector: TelegramConnector }>(
      `/agents/${encodeURIComponent(agentId)}/connectors/${encodeURIComponent(connectorId)}/credential`,
      { method: 'PUT', body: JSON.stringify({ botToken }) },
    ),
  approveTelegramPairing: (
    agentId: string,
    connectorId: string,
    chatId: string,
  ) =>
    request<{ connector: TelegramConnector }>(
      `/agents/${encodeURIComponent(agentId)}/connectors/${encodeURIComponent(connectorId)}/pairings/${encodeURIComponent(chatId)}/approve`,
      { method: 'POST' },
    ),
  restartTelegramConnector: (agentId: string, connectorId: string) =>
    request<{ connector: TelegramConnector }>(
      `/agents/${encodeURIComponent(agentId)}/connectors/${encodeURIComponent(connectorId)}/restart`,
      { method: 'POST' },
    ),
  deleteTelegramConnector: (agentId: string, connectorId: string) =>
    request<{ deleted: boolean }>(
      `/agents/${encodeURIComponent(agentId)}/connectors/${encodeURIComponent(connectorId)}`,
      { method: 'DELETE' },
    ),
  listConnectorMessages: (
    agentId: string,
    connectorId: string,
    page: { before?: string; limit?: number } = {},
  ) => {
    const query = new URLSearchParams();
    if (page.before) query.set('before', page.before);
    if (page.limit) query.set('limit', String(page.limit));
    const suffix = query.size ? `?${query}` : '';
    return request<{ messages: ConnectorMessage[]; nextBefore: string | null }>(
      `/agents/${encodeURIComponent(agentId)}/connectors/${encodeURIComponent(connectorId)}/messages${suffix}`,
    );
  },
  listSchedules: (agentId: string) =>
    request<{ schedules: DaemonSchedule[] }>(
      `/agents/${encodeURIComponent(agentId)}/schedules`,
    ),
  createSchedule: (agentId: string, input: ScheduleCreateInput) =>
    request<{ schedule: DaemonSchedule }>(
      `/agents/${encodeURIComponent(agentId)}/schedules`,
      { method: 'POST', body: JSON.stringify(input) },
    ),
  updateSchedule: (
    agentId: string,
    scheduleId: string,
    patch: Partial<
      Pick<DaemonSchedule, 'prompt' | 'trigger' | 'target' | 'enabled'>
    >,
  ) =>
    request<{ schedule: DaemonSchedule }>(
      `/agents/${encodeURIComponent(agentId)}/schedules/${encodeURIComponent(scheduleId)}`,
      { method: 'PATCH', body: JSON.stringify(patch) },
    ),
  deleteSchedule: (agentId: string, scheduleId: string) =>
    request<{ deleted: boolean }>(
      `/agents/${encodeURIComponent(agentId)}/schedules/${encodeURIComponent(scheduleId)}`,
      { method: 'DELETE' },
    ),
  importLegacySchedules: (
    agentId: string,
    input: { schedules: LegacyScheduleInput[] },
  ) =>
    request<{ schedules: DaemonSchedule[] }>(
      `/agents/${encodeURIComponent(agentId)}/schedules/import`,
      { method: 'POST', body: JSON.stringify(input) },
    ),

  getWorkspace: (): Promise<DaemonWorkspaceState> =>
    setupClient.workspace.get(),

  uploadWorkspaceAvatar: (file: File) =>
    request<void>('/workspace/avatar', {
      method: 'PUT',
      headers: { 'content-type': file.type },
      body: file,
    }),

  putWorkspace: (input: WorkspaceConfigInput): Promise<DaemonWorkspaceState> =>
    setupClient.workspace.put(input),

  validateWorkspace: (
    input: WorkspaceConfigInput,
  ): Promise<DaemonWorkspaceValidation> =>
    setupClient.workspace.validate(input),

  generateProfile: (input: GenerateProfileInput) =>
    request<{ profile: AgentProfile }>('/agents/generate-profile', {
      method: 'POST',
      body: JSON.stringify(input),
    }),

  generateAgency: (input: AgencyGenerateRequest): Promise<GeneratedAgency> =>
    setupClient.agencies.generate(input),

  bootstrapWorkspace: (
    input: BootstrapWorkspaceInput,
  ): Promise<{
    workspace: DaemonWorkspaceConfig;
    agent: DaemonSnapshot;
    workers?: DaemonSnapshot[];
  }> => setupClient.workspace.bootstrap(input),

  inspectWorkspace: async (
    rootPath: string,
  ): Promise<WorkspaceInspectResponse> => {
    const response = await setupClient.workspace.inspect(rootPath);
    if (!response.found) return { found: false };
    if (
      response.companyName === undefined ||
      !response.orchestrator ||
      !response.workers ||
      response.providerAvailable === undefined
    )
      throw new Error(
        'The workspace preview is incomplete. Inspect the folder again.',
      );
    return {
      ...response,
      found: true,
      companyName: response.companyName,
      orchestrator: response.orchestrator,
      workers: response.workers,
      providerAvailable: response.providerAvailable,
    };
  },

  pickWorkspaceFolder: () => setupClient.workspace.pickFolder(),

  resumeWorkspace: (rootPath: string): Promise<WorkspaceResumeResponse> =>
    setupClient.workspace.resume(rootPath),
};

/* ── adapters: daemon wire format → UI view model ── */

function mapRole(role: string): ChatMessage['role'] {
  switch (role) {
    case 'user':
      return 'User';
    case 'assistant':
      return 'Assistant';
    default:
      return 'System';
  }
}

function mapStatus(status: string): AgentDetail['status'] {
  switch (status) {
    case 'running':
      return 'Running';
    case 'completed':
      return 'Completed';
    case 'failed':
      return 'Failed';
    case 'terminated':
      return 'Terminated';
    default:
      return 'Idle';
  }
}

export function toAgentDetail(snapshot: DaemonSnapshot): AgentDetail {
  const { state } = snapshot;
  return {
    ...(state.config.settings?.additional?.workspaceRole === 'lead'
      ? { workspaceRole: 'lead' as const }
      : state.config.settings?.additional?.workspaceRole === 'helper'
        ? { workspaceRole: 'helper' as const }
        : {}),
    id: state.id,
    name: state.name,
    ...(state.config.bio ? { bio: state.config.bio } : {}),
    provider: state.config.provider ?? 'default',
    model: state.config.model,
    toolNames: state.config.tools?.map((tool) => tool.name) ?? [],
    created_at_ms: state.createdAtMs,
    status: mapStatus(state.status),
    token_usage: {
      prompt_tokens: state.tokenUsage.promptTokens,
      completion_tokens: state.tokenUsage.completionTokens,
      total_tokens: state.tokenUsage.totalTokens,
    },
    system: state.config.system,
    ...(state.config.fallbacks?.length
      ? { fallbacks: state.config.fallbacks }
      : {}),
    messages: snapshot.messages
      .filter((m) => {
        // hide check-in plumbing: tagged prompt messages + silent ticks
        if (m.role === 'user' && m.content.metadata?.kind === 'checkin')
          return false;
        if (m.role === 'assistant' && m.content.text.trim() === 'CHECKIN_OK')
          return false;
        return true;
      })
      .map((m) => ({
        id: m.id,
        roomId: m.roomId,
        role: mapRole(m.role),
        content: { text: m.content.text, metadata: m.content.metadata },
        created_at_ms: m.createdAtMs,
      })),
  };
}

const SESSION_ROLES = {
  user: 'User',
  assistant: 'Assistant',
  system: 'System',
  tool: 'Tool',
} as const satisfies Record<SessionMessage['role'], ChatMessage['role']>;
const CHECKIN_SUFFIX = /\n\n\(This is a scheduled check-in\.[\s\S]*\)\s*$/;

/** A session-route message as the chat components render it. A check-in
 *  prompt becomes a system line without the scheduler's instructions. */
export function toChatMessage(message: SessionMessage): ChatMessage {
  const checkin =
    message.role === 'user' && message.metadata.kind === 'checkin';
  return {
    id: message.id,
    role: checkin ? 'System' : SESSION_ROLES[message.role],
    content: {
      text: checkin ? message.text.replace(CHECKIN_SUFFIX, '') : message.text,
      ...(message.attachments.length
        ? { attachments: message.attachments }
        : {}),
      metadata: message.metadata,
    },
    created_at_ms: message.createdAtMs,
  };
}

/** Installs a shipped know-how bundle for an agent through the usual
 *  community bundle review: its skills arrive as drafts the owner approves. */
export async function installKnowHow(
  agentId: string,
  manifestText: string,
): Promise<void> {
  const review = await communityBundlesOwnerApi.review(agentId, {
    manifestText,
    programBindings: {},
  });
  await communityBundlesOwnerApi.install(agentId, {
    reviewId: review.id,
    reviewDigest: review.reviewDigest,
  });
}

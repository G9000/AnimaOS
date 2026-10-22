import {
  isRunLifecycleEvent,
  type AgentEvent,
  type LiveToolCard,
  type Run,
  type SessionMessage,
} from '@animaos/sdk';

export interface SessionStepState {
  runId: string;
  text: string;
  textOffset: number;
}

export interface SessionToolState extends LiveToolCard {
  runId: string;
}

export interface SessionEventState {
  agentId: string;
  sessionId: string;
  messages: SessionMessage[];
  runs: Run[];
  /** Includes snapshot runs outside the selected session for safe filtering. */
  runSessionIds: Record<string, string>;
  /** The agent that owns each run; helper runs may differ from the stream agent. */
  runAgentIds: Record<string, string>;
  progressByRun: Record<string, string>;
  steps: Record<string, SessionStepState>;
  tools: Record<string, SessionToolState>;
  refreshRequired: boolean;
}

const MAX_MESSAGE_ORDINAL = 18_446_744_073_709_551_615n;

function messageOrdinal(id: string): bigint {
  const separator = id.lastIndexOf('-');
  if (separator < 0 || !id.slice(0, separator).startsWith('msg-')) return 0n;

  const counter = id.slice(separator + 1);
  if (!/^\d+$/.test(counter)) return 0n;

  const ordinal = BigInt(counter);
  return ordinal <= MAX_MESSAGE_ORDINAL ? ordinal : 0n;
}

function compareMessageIds(leftId: string, rightId: string): number {
  const leftOrdinal = messageOrdinal(leftId);
  const rightOrdinal = messageOrdinal(rightId);
  if (leftOrdinal !== rightOrdinal) return leftOrdinal < rightOrdinal ? -1 : 1;
  return leftId < rightId ? -1 : leftId > rightId ? 1 : 0;
}

/** Merge live placeholders with persisted messages, preferring persisted data. */
export function mergeSessionMessages(
  current: SessionMessage[],
  authoritative: SessionMessage[],
): SessionMessage[] {
  const messagesById = new Map(current.map((message) => [message.id, message]));
  for (const message of authoritative) {
    messagesById.set(message.id, message);
  }

  return [...messagesById.values()].sort((left, right) => {
    if (left.createdAtMs !== right.createdAtMs) {
      return left.createdAtMs - right.createdAtMs;
    }
    return compareMessageIds(left.id, right.id);
  });
}

export function createSessionEventState(
  agentId: string,
  sessionId: string,
  messages: SessionMessage[] = [],
): SessionEventState {
  return {
    agentId,
    sessionId,
    messages: mergeSessionMessages([], messages),
    runs: [],
    runSessionIds: {},
    runAgentIds: {},
    progressByRun: {},
    steps: {},
    tools: {},
    refreshRequired: false,
  };
}

export function clearSessionRefreshRequest(
  state: SessionEventState,
): SessionEventState {
  return state.refreshRequired ? { ...state, refreshRequired: false } : state;
}

function sessionToolKey(
  runId: string,
  stepId: string,
  toolCallId: string,
): string {
  return JSON.stringify([runId, stepId, toolCallId]);
}

function findToolForEvent(
  state: SessionEventState,
  event: Extract<AgentEvent, { type: 'tool.started' | 'tool.finished' }>,
): SessionToolState | undefined {
  if (event.runId) {
    return state.tools[
      sessionToolKey(event.runId, event.stepId, event.toolCallId)
    ];
  }

  const matches = Object.values(state.tools).filter(
    (tool) =>
      tool.stepId === event.stepId && tool.toolCallId === event.toolCallId,
  );
  return matches.length === 1 ? matches[0] : undefined;
}

function eventRunId(
  state: SessionEventState,
  event: AgentEvent,
): string | undefined {
  if (isRunLifecycleEvent(event)) return event.run.id;
  if (event.runId) return event.runId;
  if (event.type === 'step.delta') return state.steps[event.stepId]?.runId;
  if (event.type === 'tool.started' || event.type === 'tool.finished') {
    return findToolForEvent(state, event)?.runId;
  }
  if (event.type === 'message.created' && event.stepId) {
    return state.steps[event.stepId]?.runId;
  }
  return undefined;
}

function eventSessionId(
  state: SessionEventState,
  event: AgentEvent,
): string | undefined {
  if (isRunLifecycleEvent(event)) return event.run.sessionId;
  if (event.sessionId) return event.sessionId;
  const runId = eventRunId(state, event);
  return runId ? state.runSessionIds[runId] : undefined;
}

function isEventFromSelectedStream(
  state: SessionEventState,
  event: AgentEvent,
  sourceAgentId: string,
): boolean {
  if (sourceAgentId !== state.agentId) return false;
  if (event.type === 'stream.snapshot' || event.type === 'stream.resync') {
    return event.agentId === state.agentId;
  }

  if (isRunLifecycleEvent(event)) {
    if (
      event.run.agentId !== event.agentId ||
      (event.sessionId !== undefined && event.sessionId !== event.run.sessionId)
    ) {
      return false;
    }
    const knownAgentId = state.runAgentIds[event.run.id];
    const knownSessionId = state.runSessionIds[event.run.id];
    if (knownAgentId && knownAgentId !== event.run.agentId) return false;
    if (knownSessionId && knownSessionId !== event.run.sessionId) return false;
    return (
      event.agentId === state.agentId || event.run.sessionId === state.sessionId
    );
  }

  const runId = eventRunId(state, event);
  if (event.agentId === state.agentId) {
    if (!runId) return true;
    const knownAgentId = state.runAgentIds[runId];
    const knownSessionId = state.runSessionIds[runId];
    if (knownAgentId && knownAgentId !== event.agentId) return false;
    if (
      knownSessionId &&
      event.sessionId !== undefined &&
      knownSessionId !== event.sessionId
    ) {
      return false;
    }
    return true;
  }

  return (
    runId !== undefined &&
    state.runAgentIds[runId] === event.agentId &&
    state.runSessionIds[runId] === state.sessionId &&
    (event.sessionId === undefined || event.sessionId === state.sessionId)
  );
}

function applySnapshot(
  state: SessionEventState,
  event: Extract<AgentEvent, { type: 'stream.snapshot' }>,
): SessionEventState {
  const runSessionIds: Record<string, string> = {};
  const runAgentIds: Record<string, string> = {};
  const runs: Run[] = [];
  const steps: Record<string, SessionStepState> = {};
  const tools: Record<string, SessionToolState> = {};

  for (const snapshotRun of event.runs) {
    const { run } = snapshotRun;
    runSessionIds[run.id] = run.sessionId;
    runAgentIds[run.id] = run.agentId;
    if (run.sessionId !== state.sessionId) continue;

    runs.push(run);
    if (snapshotRun.stepId) {
      steps[snapshotRun.stepId] = {
        runId: run.id,
        text: snapshotRun.text,
        textOffset: snapshotRun.textOffset,
      };
    }
    for (const tool of snapshotRun.tools) {
      tools[sessionToolKey(run.id, tool.stepId, tool.toolCallId)] = {
        ...tool,
        runId: run.id,
      };
    }
  }

  return {
    ...state,
    runs,
    runSessionIds,
    runAgentIds,
    progressByRun: {},
    steps,
    tools,
  };
}

function replaceRun(runs: Run[], updated: Run): Run[] {
  const index = runs.findIndex((run) => run.id === updated.id);
  if (index < 0) return [...runs, updated];
  return runs.map((run) => (run.id === updated.id ? updated : run));
}

export function reduceSessionEvent(
  state: SessionEventState,
  event: AgentEvent,
  sourceAgentId = state.agentId,
): SessionEventState {
  if (!isEventFromSelectedStream(state, event, sourceAgentId)) return state;

  if (event.type === 'stream.snapshot') return applySnapshot(state, event);
  if (event.type === 'stream.resync') {
    return state.refreshRequired ? state : { ...state, refreshRequired: true };
  }

  const runId = eventRunId(state, event);
  const sessionId = eventSessionId(state, event);
  if (sessionId !== state.sessionId) return state;

  if (isRunLifecycleEvent(event)) {
    return {
      ...state,
      runs: replaceRun(state.runs, event.run),
      runSessionIds: {
        ...state.runSessionIds,
        [event.run.id]: event.run.sessionId,
      },
      runAgentIds: { ...state.runAgentIds, [event.run.id]: event.run.agentId },
    };
  }

  if (event.type === 'message.created') {
    if (state.messages.some((message) => message.id === event.messageId))
      return state;
    const placeholder: SessionMessage = {
      id: event.messageId,
      role: event.role,
      text: '',
      attachments: [],
      metadata: {},
      createdAtMs: event.at,
    };
    return {
      ...state,
      messages: mergeSessionMessages(state.messages, [placeholder]),
    };
  }

  if (!runId) return state;

  if (event.type === 'run.progress') {
    if (state.progressByRun[runId] === event.phase) return state;
    return {
      ...state,
      progressByRun: { ...state.progressByRun, [runId]: event.phase },
    };
  }

  if (event.type === 'step.delta') {
    const previous = state.steps[event.stepId];
    const textOffset = previous?.textOffset ?? event.offset;
    const text = previous?.text ?? '';
    if (previous && previous.runId !== runId) return state;
    if (event.offset < textOffset) return state;

    const relativeOffset = event.offset - textOffset;
    if (relativeOffset > text.length) return state;
    const nextText =
      text.slice(0, relativeOffset) +
      event.text +
      text.slice(relativeOffset + event.text.length);
    if (previous?.text === nextText) return state;

    return {
      ...state,
      steps: {
        ...state.steps,
        [event.stepId]: { runId, text: nextText, textOffset },
      },
    };
  }

  if (event.type === 'tool.started') {
    const tool: SessionToolState = {
      runId,
      stepId: event.stepId,
      toolCallId: event.toolCallId,
      name: event.name,
      argumentsPreview: event.argumentsPreview,
      argumentsTruncated: event.argumentsTruncated,
      status: 'running',
      durationMs: null,
      resultPreview: null,
      truncated: false,
    };
    const key = sessionToolKey(runId, event.stepId, event.toolCallId);
    return { ...state, tools: { ...state.tools, [key]: tool } };
  }

  if (event.type === 'tool.finished') {
    const key = sessionToolKey(runId, event.stepId, event.toolCallId);
    const previous = state.tools[key];
    const tool: SessionToolState = {
      runId,
      stepId: event.stepId,
      toolCallId: event.toolCallId,
      name: event.name,
      argumentsPreview: previous?.argumentsPreview ?? '',
      argumentsTruncated: previous?.argumentsTruncated ?? false,
      status: event.status,
      durationMs: event.durationMs,
      resultPreview: event.resultPreview,
      truncated: event.truncated,
      ...(event.sources ? { sources: event.sources } : {}),
    };
    return { ...state, tools: { ...state.tools, [key]: tool } };
  }

  return state;
}

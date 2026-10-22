import { describe, expect, it } from 'vitest';
import type { AgentEvent, Run, SessionMessage } from '@animaos/sdk';
import {
  createSessionEventState,
  mergeSessionMessages,
  reduceSessionEvent,
} from './session-state.js';

function createRun(
  id: string,
  sessionId: string,
  overrides: Partial<Run> = {},
): Run {
  return {
    id,
    agentId: 'agent-a',
    sessionId,
    source: 'web',
    sourceRef: null,
    idempotencyKey: null,
    status: 'running',
    input: { text: 'hello', attachmentIds: [], skill: null },
    createdAtMs: 10,
    startedAtMs: 10,
    finishedAtMs: null,
    error: null,
    stop: null,
    toolsStarted: [],
    steps: [],
    usage: { promptTokens: 0, completionTokens: 0, totalTokens: 0 },
    model: 'test-model',
    provider: 'test-provider',
    parentRunId: null,
    replyMessageId: null,
    ...overrides,
  };
}

function createMessage(
  id: string,
  text: string,
  createdAtMs: number,
  role: SessionMessage['role'] = 'assistant',
): SessionMessage {
  return {
    id,
    role,
    text,
    attachments: [],
    metadata: {},
    createdAtMs,
  };
}

function eventBase(overrides: Partial<AgentEvent> = {}) {
  return {
    agentId: 'agent-a',
    seq: 1,
    at: 100,
    ...overrides,
  };
}

function findTool(
  state: ReturnType<typeof createSessionEventState>,
  stepId: string,
  toolCallId: string,
) {
  return Object.values(state.tools).find(
    (tool) => tool.stepId === stepId && tool.toolCallId === toolCallId,
  );
}

describe('session state helpers', () => {
  it('merges messages by id and sorts by time, numeric ordinal, then id', () => {
    const pendingDuplicate = createMessage('msg-1700-2', '', 10);
    const current = [
      createMessage('msg-1700-10', 'ten', 10),
      pendingDuplicate,
      createMessage('msg-c', 'c', 20),
      createMessage('other-z', 'non-runtime z', 10),
      createMessage('other-a', 'non-runtime a', 10),
      createMessage('msg-1800-2', 'same ordinal, later id', 10),
    ];
    const authoritativeDuplicate = createMessage(
      'msg-1700-2',
      'persisted answer',
      10,
    );
    const authoritative = [
      authoritativeDuplicate,
      createMessage('other-m', 'non-runtime m', 10),
    ];

    const merged = mergeSessionMessages(current, authoritative);

    expect(merged.map(({ id }) => id)).toEqual([
      'other-a',
      'other-m',
      'other-z',
      'msg-1700-2',
      'msg-1800-2',
      'msg-1700-10',
      'msg-c',
    ]);
    expect(merged.find(({ id }) => id === 'msg-1700-2')).toBe(
      authoritativeDuplicate,
    );
  });

  it('ignores events from another agent or session', () => {
    const state = createSessionEventState('agent-a', 'session-a');
    const otherSessionSnapshot = {
      ...eventBase({ agentId: 'agent-b' }),
      type: 'stream.snapshot',
      runs: [
        {
          run: createRun('run-other', 'session-a', { agentId: 'agent-b' }),
          stepId: 'step-other',
          text: 'ignored',
          textOffset: 0,
          tools: [],
        },
      ],
      approvals: [],
    } satisfies AgentEvent;
    const otherSessionMessage = {
      ...eventBase({ sessionId: 'session-b' }),
      type: 'message.created',
      messageId: 'msg-other',
      role: 'assistant',
      stepId: null,
    } satisfies AgentEvent;

    const afterOtherAgent = reduceSessionEvent(state, otherSessionSnapshot);
    const afterOtherSession = reduceSessionEvent(
      afterOtherAgent,
      otherSessionMessage,
    );

    expect(afterOtherAgent).toBe(state);
    expect(afterOtherSession).toBe(state);
    expect(afterOtherSession.messages).toEqual([]);
    expect(afterOtherSession.runs).toEqual([]);
  });

  it('deduplicates repeated message.created events and reconciles their placeholders', () => {
    const state = createSessionEventState('agent-a', 'session-a');
    const firstMessageEvent = {
      ...eventBase({ sessionId: 'session-a', seq: 1, at: 101 }),
      type: 'message.created',
      messageId: 'msg-live',
      role: 'assistant',
      stepId: 'step-a',
    } satisfies AgentEvent;
    const repeatedMessageEvent = { ...firstMessageEvent, seq: 2, at: 102 };

    const afterFirst = reduceSessionEvent(state, firstMessageEvent);
    const afterRepeated = reduceSessionEvent(afterFirst, repeatedMessageEvent);
    const persisted = createMessage(
      'msg-live',
      'complete persisted answer',
      101,
    );
    const reconciled = {
      ...afterRepeated,
      messages: mergeSessionMessages(afterRepeated.messages, [persisted]),
    };
    const afterReplay = reduceSessionEvent(reconciled, repeatedMessageEvent);

    expect(afterRepeated.messages).toHaveLength(1);
    expect(afterReplay.messages).toEqual([persisted]);
  });

  it('marks stream.resync as a request to refresh authoritative state', () => {
    const state = createSessionEventState('agent-a', 'session-a');

    const refreshed = reduceSessionEvent(state, {
      ...eventBase({ seq: 2 }),
      type: 'stream.resync',
      missed: 3,
    });

    expect(refreshed.refreshRequired).toBe(true);
  });

  it('seeds only the selected session runs, step text, and tools from a snapshot', () => {
    const state = createSessionEventState('agent-a', 'session-a');
    const snapshot = {
      ...eventBase(),
      type: 'stream.snapshot',
      runs: [
        {
          run: createRun('run-a', 'session-a'),
          stepId: 'step-a',
          text: 'hello',
          textOffset: 4,
          tools: [
            {
              stepId: 'step-a',
              toolCallId: 'tool-a',
              name: 'memory_search',
              argumentsPreview: '{"query":"hello"}',
              argumentsTruncated: false,
              status: 'running',
              durationMs: null,
              resultPreview: null,
              truncated: false,
            },
          ],
        },
        {
          run: createRun('run-b', 'session-b'),
          stepId: 'step-b',
          text: 'unrelated',
          textOffset: 0,
          tools: [],
        },
        {
          run: createRun('run-helper', 'session-a', {
            agentId: 'agent-helper',
          }),
          stepId: 'step-helper',
          text: 'helper progress',
          textOffset: 0,
          tools: [
            {
              stepId: 'step-helper',
              toolCallId: 'helper-tool',
              name: 'delegate',
              argumentsPreview: '{}',
              argumentsTruncated: false,
              status: 'running',
              durationMs: null,
              resultPreview: null,
              truncated: false,
            },
          ],
        },
      ],
      approvals: [],
    } satisfies AgentEvent;

    const seeded = reduceSessionEvent(state, snapshot);

    expect(seeded.runs.map(({ id }) => id)).toEqual(['run-a', 'run-helper']);
    expect(seeded.runSessionIds).toEqual({
      'run-a': 'session-a',
      'run-b': 'session-b',
      'run-helper': 'session-a',
    });
    expect(seeded.steps['step-a']).toEqual({
      runId: 'run-a',
      text: 'hello',
      textOffset: 4,
    });
    expect(seeded.steps['step-b']).toBeUndefined();
    expect(seeded.steps['step-helper']).toEqual({
      runId: 'run-helper',
      text: 'helper progress',
      textOffset: 0,
    });
    expect(findTool(seeded, 'step-a', 'tool-a')).toMatchObject({
      status: 'running',
      name: 'memory_search',
    });
    expect(findTool(seeded, 'step-helper', 'helper-tool')).toMatchObject({
      runId: 'run-helper',
      status: 'running',
      name: 'delegate',
    });
  });

  it('attributes sessionless run events through the snapshot run map', () => {
    const state = createSessionEventState('agent-a', 'session-a');
    const seeded = reduceSessionEvent(state, {
      ...eventBase(),
      type: 'stream.snapshot',
      runs: [
        {
          run: createRun('run-a', 'session-a'),
          stepId: 'step-a',
          text: 'Hi',
          textOffset: 0,
          tools: [],
        },
        {
          run: createRun('run-b', 'session-b'),
          stepId: 'step-b',
          text: '',
          textOffset: 0,
          tools: [],
        },
      ],
      approvals: [],
    } satisfies AgentEvent);
    const lifecycle = reduceSessionEvent(seeded, {
      ...eventBase({ seq: 2 }),
      type: 'run.awaiting_approval',
      run: createRun('run-a', 'session-a', { status: 'awaiting_approval' }),
    });
    const progress = reduceSessionEvent(lifecycle, {
      ...eventBase({ seq: 3, runId: 'run-a' }),
      type: 'run.progress',
      phase: 'thinking',
    });
    const step = reduceSessionEvent(progress, {
      ...eventBase({ seq: 4, runId: 'run-a' }),
      type: 'step.delta',
      stepId: 'step-a',
      offset: 2,
      text: ' there',
    });
    const started = reduceSessionEvent(step, {
      ...eventBase({ seq: 5, runId: 'run-a' }),
      type: 'tool.started',
      stepId: 'step-a',
      toolCallId: 'tool-new',
      name: 'web_search',
      argumentsPreview: '{"q":"news"}',
      argumentsTruncated: false,
    });
    const finished = reduceSessionEvent(started, {
      ...eventBase({ seq: 6, runId: 'run-a' }),
      type: 'tool.finished',
      stepId: 'step-a',
      toolCallId: 'tool-new',
      name: 'web_search',
      status: 'success',
      durationMs: 25,
      resultPreview: 'result',
      truncated: false,
      recovered: false,
    });
    const otherRunProgress = reduceSessionEvent(finished, {
      ...eventBase({ seq: 7, runId: 'run-b' }),
      type: 'run.progress',
      phase: 'must be ignored',
    });

    expect(lifecycle.runs[0].status).toBe('awaiting_approval');
    expect(finished.progressByRun['run-a']).toBe('thinking');
    expect(finished.steps['step-a']).toEqual({
      runId: 'run-a',
      text: 'Hi there',
      textOffset: 0,
    });
    expect(findTool(finished, 'step-a', 'tool-new')).toMatchObject({
      runId: 'run-a',
      status: 'success',
      resultPreview: 'result',
      durationMs: 25,
    });
    expect(otherRunProgress).toBe(finished);
  });

  it('accepts helper-run follow-up events from the selected agent stream', () => {
    const state = createSessionEventState('agent-a', 'session-a');
    const helperRun = createRun('run-helper', 'session-a', {
      agentId: 'agent-helper',
    });
    const seeded = reduceSessionEvent(state, {
      ...eventBase(),
      type: 'stream.snapshot',
      runs: [
        {
          run: helperRun,
          stepId: 'helper-step',
          text: 'helper',
          textOffset: 0,
          tools: [],
        },
      ],
      approvals: [],
    } satisfies AgentEvent);
    const lifecycle = reduceSessionEvent(
      seeded,
      {
        ...eventBase({ agentId: 'agent-helper', seq: 2 }),
        type: 'run.awaiting_approval',
        run: createRun('run-helper', 'session-a', {
          agentId: 'agent-helper',
          status: 'awaiting_approval',
        }),
      },
      'agent-a',
    );
    const progress = reduceSessionEvent(
      lifecycle,
      {
        ...eventBase({ agentId: 'agent-helper', seq: 3, runId: 'run-helper' }),
        type: 'run.progress',
        phase: 'waiting for approval',
      },
      'agent-a',
    );
    const delta = reduceSessionEvent(
      progress,
      {
        ...eventBase({ agentId: 'agent-helper', seq: 4, runId: 'run-helper' }),
        type: 'step.delta',
        stepId: 'helper-step',
        offset: 6,
        text: ' work',
      },
      'agent-a',
    );
    const started = reduceSessionEvent(
      delta,
      {
        ...eventBase({ agentId: 'agent-helper', seq: 5, runId: 'run-helper' }),
        type: 'tool.started',
        stepId: 'helper-step',
        toolCallId: 'helper-tool',
        name: 'delegate',
        argumentsPreview: '{}',
        argumentsTruncated: false,
      },
      'agent-a',
    );
    const finished = reduceSessionEvent(
      started,
      {
        ...eventBase({ agentId: 'agent-helper', seq: 6 }),
        type: 'tool.finished',
        stepId: 'helper-step',
        toolCallId: 'helper-tool',
        name: 'delegate',
        status: 'success',
        durationMs: 20,
        resultPreview: 'done',
        truncated: false,
        recovered: false,
      },
      'agent-a',
    );

    expect(lifecycle.runs[0].status).toBe('awaiting_approval');
    expect(progress.progressByRun['run-helper']).toBe('waiting for approval');
    expect(delta.steps['helper-step'].text).toBe('helper work');
    expect(findTool(finished, 'helper-step', 'helper-tool')).toMatchObject({
      runId: 'run-helper',
      status: 'success',
      resultPreview: 'done',
    });
  });

  it('associates a new helper run from its lifecycle event on the selected stream', () => {
    const state = createSessionEventState('agent-a', 'session-a');
    const lifecycle = reduceSessionEvent(
      state,
      {
        ...eventBase({ agentId: 'agent-helper' }),
        type: 'run.started',
        run: createRun('run-new-helper', 'session-a', {
          agentId: 'agent-helper',
        }),
      },
      'agent-a',
    );
    const progress = reduceSessionEvent(
      lifecycle,
      {
        ...eventBase({
          agentId: 'agent-helper',
          seq: 2,
          runId: 'run-new-helper',
        }),
        type: 'run.progress',
        phase: 'working',
      },
      'agent-a',
    );

    expect(lifecycle.runs.map(({ id }) => id)).toEqual(['run-new-helper']);
    expect(lifecycle.runSessionIds['run-new-helper']).toBe('session-a');
    expect(lifecycle.runAgentIds['run-new-helper']).toBe('agent-helper');
    expect(progress.progressByRun['run-new-helper']).toBe('working');
  });

  it('rejects helper events from another source stream or session', () => {
    const state = createSessionEventState('agent-a', 'session-a');
    const seeded = reduceSessionEvent(state, {
      ...eventBase(),
      type: 'stream.snapshot',
      runs: [
        {
          run: createRun('run-helper', 'session-a', {
            agentId: 'agent-helper',
          }),
          stepId: null,
          text: '',
          textOffset: 0,
          tools: [],
        },
      ],
      approvals: [],
    } satisfies AgentEvent);
    const otherSource = reduceSessionEvent(
      seeded,
      {
        ...eventBase({ sessionId: 'session-a', seq: 2 }),
        type: 'message.created',
        messageId: 'source-message',
        role: 'assistant',
        stepId: null,
      },
      'agent-other',
    );
    const otherSession = reduceSessionEvent(
      seeded,
      {
        ...eventBase({
          agentId: 'agent-helper',
          sessionId: 'session-b',
          runId: 'run-helper',
          seq: 3,
        }),
        type: 'run.progress',
        phase: 'must be ignored',
      },
      'agent-a',
    );
    const unassociatedAgent = reduceSessionEvent(
      seeded,
      {
        ...eventBase({
          agentId: 'unassociated-helper',
          sessionId: 'session-a',
          runId: 'unknown-run',
          seq: 4,
        }),
        type: 'run.progress',
        phase: 'must also be ignored',
      },
      'agent-a',
    );

    expect(otherSource).toBe(seeded);
    expect(otherSession).toBe(seeded);
    expect(unassociatedAgent).toBe(seeded);
  });

  it('keeps reused tool call ids distinct and finishes the matching step', () => {
    const state = createSessionEventState('agent-a', 'session-a');
    const seeded = reduceSessionEvent(state, {
      ...eventBase(),
      type: 'stream.snapshot',
      runs: [
        {
          run: createRun('run-a', 'session-a'),
          stepId: null,
          text: '',
          textOffset: 0,
          tools: [],
        },
      ],
      approvals: [],
    } satisfies AgentEvent);
    const firstStep = reduceSessionEvent(seeded, {
      ...eventBase({ seq: 2, runId: 'run-a' }),
      type: 'tool.started',
      stepId: 'step-1',
      toolCallId: 'provider-call-1',
      name: 'search',
      argumentsPreview: '{"query":"first"}',
      argumentsTruncated: false,
    });
    const secondStep = reduceSessionEvent(firstStep, {
      ...eventBase({ seq: 3, runId: 'run-a' }),
      type: 'tool.started',
      stepId: 'step-2',
      toolCallId: 'provider-call-1',
      name: 'search',
      argumentsPreview: '{"query":"second"}',
      argumentsTruncated: false,
    });
    const finishedFirstStep = reduceSessionEvent(secondStep, {
      ...eventBase({ seq: 4 }),
      type: 'tool.finished',
      stepId: 'step-1',
      toolCallId: 'provider-call-1',
      name: 'search',
      status: 'success',
      durationMs: 15,
      resultPreview: 'first result',
      truncated: false,
      recovered: false,
    });

    expect(Object.values(finishedFirstStep.tools)).toHaveLength(2);
    expect(
      findTool(finishedFirstStep, 'step-1', 'provider-call-1'),
    ).toMatchObject({
      status: 'success',
      argumentsPreview: '{"query":"first"}',
      resultPreview: 'first result',
    });
    expect(
      findTool(finishedFirstStep, 'step-2', 'provider-call-1'),
    ).toMatchObject({
      status: 'running',
      argumentsPreview: '{"query":"second"}',
      resultPreview: null,
    });
  });
});

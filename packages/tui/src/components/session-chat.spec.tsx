import { afterEach, describe, expect, it, vi } from 'vitest';
import type {
  AgentEvent,
  Run,
  RunMode,
  Session,
  SessionMessage,
} from '@animaos/sdk';
import {
  createSessionEventState,
  reduceSessionEvent,
} from '../session-state.js';
import {
  cleanupInk,
  flushInk,
  pressInkKey,
  renderInk,
  submitInk,
} from '../test-harness.js';
import { SessionChat } from './session-chat.js';

afterEach(() => {
  cleanupInk();
  vi.clearAllMocks();
});

function createSession(): Session {
  return {
    id: 'session-a',
    agentId: 'agent-a',
    roomId: 'session-a',
    kind: 'chat',
    origin: 'web',
    title: 'Release planning',
    titleSource: 'owner',
    createdAtMs: 1,
    lastActivityAtMs: 1,
    lastReadAtMs: null,
    archived: false,
    parentSessionId: null,
    parentRunId: null,
    parentAgentId: null,
    summary: null,
    contextTrimmed: null,
    compactionError: null,
    messageCount: 0,
    preview: null,
    activeRuns: 1,
    pendingApprovals: 0,
    unread: false,
    capabilities: {
      send: true,
      steer: true,
      stop: true,
      rename: true,
      archive: true,
      delete: true,
      compact: true,
      export: true,
    },
  };
}

function createMessage(
  id: string,
  role: SessionMessage['role'],
  text: string,
  createdAtMs: number,
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

function createRun(status: Run['status'] = 'running'): Run {
  return {
    id: 'run-a',
    agentId: 'agent-a',
    sessionId: 'session-a',
    source: 'web',
    sourceRef: null,
    idempotencyKey: null,
    status,
    input: { text: 'hello', attachmentIds: [], skill: null },
    createdAtMs: 1,
    startedAtMs: 1,
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
  };
}

function createLiveState() {
  let state = createSessionEventState('agent-a', 'session-a');
  const base = {
    agentId: 'agent-a',
    sessionId: 'session-a',
    runId: 'run-a',
    seq: 1,
    at: 10,
  };
  const events: AgentEvent[] = [
    { ...base, type: 'run.progress', phase: 'Thinking through the plan' },
    {
      ...base,
      type: 'step.delta',
      stepId: 'step-a',
      offset: 0,
      text: 'Draft reply',
    },
    {
      ...base,
      type: 'tool.started',
      stepId: 'step-a',
      toolCallId: 'tool-a',
      name: 'web_search',
      argumentsPreview: '{ query: "release notes" }',
      argumentsTruncated: false,
    },
  ];
  state = {
    ...state,
    runSessionIds: { 'run-a': 'session-a' },
    runAgentIds: { 'run-a': 'agent-a' },
  };
  for (const event of events) state = reduceSessionEvent(state, event);
  return state;
}

describe('SessionChat', () => {
  it('renders messages in chronological order and shows streamed text and tool progress', () => {
    const messages = [
      createMessage('assistant-2', 'assistant', 'A response', 3),
      createMessage('user-1', 'user', 'A request', 1),
      createMessage('tool-1', 'tool', 'Found three pages', 2),
      createMessage('msg-run-10', 'assistant', 'Message ten', 4),
      createMessage('msg-run-2', 'assistant', 'Message two', 4),
    ];
    const rendered = renderInk(
      <SessionChat
        session={createSession()}
        messages={messages}
        runs={[createRun()]}
        activeRun={createRun()}
        eventState={createLiveState()}
        runMode="queue"
        canStopActiveRun={true}
        stopRequested={false}
        onSubmit={vi.fn()}
        onModeChange={vi.fn()}
        onStop={vi.fn()}
      />,
    );

    const frame = rendered.lastFrame();
    expect(frame.indexOf('A request')).toBeLessThan(
      frame.indexOf('Found three pages'),
    );
    expect(frame.indexOf('Found three pages')).toBeLessThan(
      frame.indexOf('A response'),
    );
    expect(frame.indexOf('Message two')).toBeLessThan(
      frame.indexOf('Message ten'),
    );
    expect(frame).toContain('Run: running');
    expect(frame).toContain('Thinking through the plan');
    expect(frame).toContain('Draft reply');
    expect(frame).toContain('web_search');
    expect(frame).toContain('Release planning');
  });

  it('shows the mode, toggles queue/steer, and submits text', async () => {
    const onSubmit = vi.fn();
    const onModeChange = vi.fn();
    const rendered = renderInk(
      <SessionChat
        session={createSession()}
        messages={[]}
        runs={[]}
        activeRun={null}
        eventState={null}
        runMode="queue"
        canStopActiveRun={false}
        stopRequested={false}
        onSubmit={onSubmit}
        onModeChange={onModeChange}
        onStop={vi.fn()}
      />,
    );

    expect(rendered.lastFrame()).toContain('Mode: queue');
    await pressInkKey(rendered, '\t');
    expect(onModeChange).toHaveBeenCalledWith('steer' satisfies RunMode);

    const sessionWithoutSteer = createSession();
    sessionWithoutSteer.capabilities.steer = false;
    rendered.rerender(
      <SessionChat
        session={sessionWithoutSteer}
        messages={[]}
        runs={[]}
        activeRun={null}
        eventState={null}
        runMode="steer"
        canStopActiveRun={false}
        stopRequested={false}
        onSubmit={onSubmit}
        onModeChange={onModeChange}
        onStop={vi.fn()}
      />,
    );
    await pressInkKey(rendered, '\t');
    expect(onModeChange).toHaveBeenLastCalledWith('queue');
    expect(rendered.lastFrame()).toContain('Steer mode is unavailable');

    rendered.rerender(
      <SessionChat
        session={createSession()}
        messages={[]}
        runs={[]}
        activeRun={null}
        eventState={null}
        runMode="queue"
        canStopActiveRun={false}
        stopRequested={false}
        onSubmit={onSubmit}
        onModeChange={onModeChange}
        onStop={vi.fn()}
      />,
    );
    await submitInk(rendered, 'continue the release notes');
    expect(onSubmit).toHaveBeenCalledWith('continue the release notes');
  });

  it('keeps a rejected draft available for correction and retry', async () => {
    const onSubmit = vi.fn().mockResolvedValue(false);
    const rendered = renderInk(
      <SessionChat
        session={createSession()}
        messages={[]}
        runs={[]}
        activeRun={null}
        eventState={null}
        runMode="queue"
        canStopActiveRun={false}
        stopRequested={false}
        onSubmit={onSubmit}
        onModeChange={vi.fn()}
        onStop={vi.fn()}
      />,
    );

    await submitInk(rendered, 'please retry this');
    await flushInk();

    expect(onSubmit).toHaveBeenCalledWith('please retry this');
    expect(rendered.lastFrame()).toContain('please retry this');
  });

  it('hides live-only progress once the current run reaches a terminal event', () => {
    const terminalRun = createRun('completed');
    const terminalEvent: AgentEvent = {
      type: 'run.completed',
      agentId: 'agent-a',
      sessionId: 'session-a',
      runId: 'run-a',
      seq: 2,
      at: 20,
      run: terminalRun,
    };
    const terminalState = reduceSessionEvent(createLiveState(), terminalEvent);
    const rendered = renderInk(
      <SessionChat
        session={createSession()}
        messages={[]}
        runs={[terminalRun]}
        activeRun={null}
        eventState={terminalState}
        runMode="queue"
        canStopActiveRun={false}
        stopRequested={false}
        onSubmit={vi.fn()}
        onModeChange={vi.fn()}
        onStop={vi.fn()}
      />,
    );

    expect(rendered.lastFrame()).toContain('Run: completed');
    expect(rendered.lastFrame()).not.toContain('Live assistant output');
    expect(rendered.lastFrame()).not.toContain('Thinking through the plan');
    expect(rendered.lastFrame()).not.toContain('Draft reply');
    expect(rendered.lastFrame()).not.toContain('Tool activity');
  });

  it('offers stop only for a stoppable run and labels a pending request accurately', async () => {
    const onStop = vi.fn();
    const rendered = renderInk(
      <SessionChat
        session={createSession()}
        messages={[]}
        runs={[createRun()]}
        activeRun={createRun()}
        eventState={null}
        runMode="queue"
        canStopActiveRun={true}
        stopRequested={false}
        onSubmit={vi.fn()}
        onModeChange={vi.fn()}
        onStop={onStop}
      />,
    );
    expect(rendered.lastFrame().toLowerCase()).toContain('ctrl+x stop');

    await pressInkKey(rendered, '\u0018');
    expect(onStop).toHaveBeenCalledTimes(1);

    rendered.rerender(
      <SessionChat
        session={createSession()}
        messages={[]}
        runs={[createRun()]}
        activeRun={createRun()}
        eventState={null}
        runMode="queue"
        canStopActiveRun={false}
        stopRequested={true}
        onSubmit={vi.fn()}
        onModeChange={vi.fn()}
        onStop={onStop}
      />,
    );
    expect(rendered.lastFrame().toLowerCase()).toContain('stop requested');
    expect(rendered.lastFrame().toLowerCase()).not.toContain('stopped');
    await pressInkKey(rendered, '\u0018');
    expect(onStop).toHaveBeenCalledTimes(1);

    rendered.rerender(
      <SessionChat
        session={createSession()}
        messages={[]}
        runs={[createRun('completed')]}
        activeRun={createRun('completed')}
        eventState={null}
        runMode="queue"
        canStopActiveRun={true}
        stopRequested={false}
        onSubmit={vi.fn()}
        onModeChange={vi.fn()}
        onStop={onStop}
      />,
    );
    expect(rendered.lastFrame().toLowerCase()).not.toContain('ctrl+x stop');
    await pressInkKey(rendered, '\u0018');
    expect(onStop).toHaveBeenCalledTimes(1);
    expect(rendered.lastFrame().toLowerCase()).not.toContain('stop requested');
  });
});

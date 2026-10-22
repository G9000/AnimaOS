import { afterEach, describe, expect, it, vi } from 'vitest';
import type { AgentSummary, Session } from '@animaos/sdk';
import { cleanupInk, pressInkKey, renderInk } from '../test-harness.js';
import { SessionList } from './session-list.js';

afterEach(() => {
  cleanupInk();
  vi.clearAllMocks();
});

function createAgent(id: string): AgentSummary {
  return {
    state: {
      id,
      name: id,
      status: 'idle',
      config: {} as AgentSummary['state']['config'],
      createdAtMs: 1,
      tokenUsage: { promptTokens: 0, completionTokens: 0, totalTokens: 0 },
    },
    messageCount: 0,
    eventCount: 0,
    lastTask: null,
  };
}

function createSession(id: string, overrides: Partial<Session> = {}): Session {
  return {
    id,
    agentId: 'agent-a',
    roomId: id,
    kind: 'chat',
    origin: 'web',
    title: id,
    titleSource: 'owner',
    createdAtMs: Date.parse('2026-10-20T00:00:00.000Z'),
    lastActivityAtMs: Date.parse('2026-10-22T11:30:00.000Z'),
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
    activeRuns: 0,
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
    ...overrides,
  };
}

describe('SessionList', () => {
  it('shows session title, activity, active run and pending approval counts', () => {
    const rendered = renderInk(
      <SessionList
        agents={[createAgent('agent-a')]}
        selectedAgentId="agent-a"
        sessions={[
          createSession('session-a', {
            title: 'Release planning',
            activeRuns: 2,
            pendingApprovals: 1,
          }),
          createSession('archived', { archived: true, title: 'Old draft' }),
          createSession('helper', { kind: 'helper', title: 'Helper work' }),
        ]}
        nextCursor={null}
        onSelectAgent={vi.fn()}
        onOpenSession={vi.fn()}
        onCreateSession={vi.fn()}
        onLoadNext={vi.fn()}
        onBack={vi.fn()}
      />,
    );

    expect(rendered.lastFrame()).toContain('Release planning');
    expect(rendered.lastFrame()).toContain('2026-10-22');
    expect(rendered.lastFrame()).toContain('2 active runs');
    expect(rendered.lastFrame()).toContain('1 pending approval');
    expect(rendered.lastFrame()).not.toContain('Old draft');
    expect(rendered.lastFrame()).not.toContain('Helper work');
  });

  it('opens the keyboard-selected session and creates a session from the list', async () => {
    const first = createSession('session-a', { title: 'First' });
    const second = createSession('session-b', { title: 'Second' });
    const onOpenSession = vi.fn();
    const onCreateSession = vi.fn();
    const rendered = renderInk(
      <SessionList
        agents={[createAgent('agent-a')]}
        selectedAgentId="agent-a"
        sessions={[first, second]}
        nextCursor={null}
        onSelectAgent={vi.fn()}
        onOpenSession={onOpenSession}
        onCreateSession={onCreateSession}
        onLoadNext={vi.fn()}
        onBack={vi.fn()}
      />,
    );

    await pressInkKey(rendered, '\u001B[B');
    await pressInkKey(rendered, '\r');
    expect(onOpenSession).toHaveBeenCalledWith(second);

    await pressInkKey(rendered, 'c');
    expect(onCreateSession).toHaveBeenCalledTimes(1);
  });

  it('requires an explicit agent choice when there are multiple agents', async () => {
    const onSelectAgent = vi.fn();
    const rendered = renderInk(
      <SessionList
        agents={[createAgent('agent-a'), createAgent('agent-b')]}
        selectedAgentId={null}
        sessions={[]}
        nextCursor={null}
        onSelectAgent={onSelectAgent}
        onOpenSession={vi.fn()}
        onCreateSession={vi.fn()}
        onLoadNext={vi.fn()}
        onBack={vi.fn()}
      />,
    );

    expect(rendered.lastFrame()).toContain('Choose an agent');
    await pressInkKey(rendered, '\u001B[B');
    await pressInkKey(rendered, '\r');

    expect(onSelectAgent).toHaveBeenCalledWith('agent-b');
  });

  it('keeps the chosen agent identity when the agent list is reordered', async () => {
    const onSelectAgent = vi.fn();
    const agents = [createAgent('agent-a'), createAgent('agent-b')];
    const rendered = renderInk(
      <SessionList
        agents={agents}
        selectedAgentId={null}
        sessions={[]}
        nextCursor={null}
        onSelectAgent={onSelectAgent}
        onOpenSession={vi.fn()}
        onCreateSession={vi.fn()}
        onLoadNext={vi.fn()}
        onBack={vi.fn()}
      />,
    );

    await pressInkKey(rendered, '\u001B[B');
    rendered.rerender(
      <SessionList
        agents={[agents[1]!, agents[0]!]}
        selectedAgentId={null}
        sessions={[]}
        nextCursor={null}
        onSelectAgent={onSelectAgent}
        onOpenSession={vi.fn()}
        onCreateSession={vi.fn()}
        onLoadNext={vi.fn()}
        onBack={vi.fn()}
      />,
    );
    await pressInkKey(rendered, '\r');

    expect(onSelectAgent).toHaveBeenCalledWith('agent-b');
  });

  it('highlights a newly selected agent when the selection prop changes', async () => {
    const agents = [createAgent('agent-a'), createAgent('agent-b')];
    const onSelectAgent = vi.fn();
    const rendered = renderInk(
      <SessionList
        agents={agents}
        selectedAgentId="agent-a"
        sessions={[]}
        nextCursor={null}
        onSelectAgent={onSelectAgent}
        onOpenSession={vi.fn()}
        onCreateSession={vi.fn()}
        onLoadNext={vi.fn()}
        onBack={vi.fn()}
      />,
    );

    rendered.rerender(
      <SessionList
        agents={agents}
        selectedAgentId="agent-b"
        sessions={[]}
        nextCursor={null}
        onSelectAgent={onSelectAgent}
        onOpenSession={vi.fn()}
        onCreateSession={vi.fn()}
        onLoadNext={vi.fn()}
        onBack={vi.fn()}
      />,
    );
    await pressInkKey(rendered, 'a');
    await pressInkKey(rendered, '\r');

    expect(onSelectAgent).toHaveBeenCalledWith('agent-b');
    expect(onSelectAgent).not.toHaveBeenCalledWith('agent-a');
  });

  it('preserves the selected session through reorder and resets safely when it is replaced', async () => {
    const first = createSession('session-a', { title: 'First' });
    const second = createSession('session-b', { title: 'Second' });
    const replacement = createSession('session-c', { title: 'Replacement' });
    const onOpenSession = vi.fn();
    const rendered = renderInk(
      <SessionList
        agents={[createAgent('agent-a')]}
        selectedAgentId="agent-a"
        sessions={[first, second]}
        nextCursor={null}
        onSelectAgent={vi.fn()}
        onOpenSession={onOpenSession}
        onCreateSession={vi.fn()}
        onLoadNext={vi.fn()}
        onBack={vi.fn()}
      />,
    );

    await pressInkKey(rendered, '\u001B[B');
    rendered.rerender(
      <SessionList
        agents={[createAgent('agent-a')]}
        selectedAgentId="agent-a"
        sessions={[second, first]}
        nextCursor={null}
        onSelectAgent={vi.fn()}
        onOpenSession={onOpenSession}
        onCreateSession={vi.fn()}
        onLoadNext={vi.fn()}
        onBack={vi.fn()}
      />,
    );
    await pressInkKey(rendered, '\r');
    expect(onOpenSession).toHaveBeenLastCalledWith(second);

    rendered.rerender(
      <SessionList
        agents={[createAgent('agent-a')]}
        selectedAgentId="agent-a"
        sessions={[first, replacement]}
        nextCursor={null}
        onSelectAgent={vi.fn()}
        onOpenSession={onOpenSession}
        onCreateSession={vi.fn()}
        onLoadNext={vi.fn()}
        onBack={vi.fn()}
      />,
    );
    await pressInkKey(rendered, '\r');
    expect(onOpenSession).toHaveBeenLastCalledWith(first);
  });

  it('renders loading, recoverable error, empty, and pagination states', async () => {
    const onLoadNext = vi.fn();
    const onRetry = vi.fn();
    const rendered = renderInk(
      <SessionList
        agents={[createAgent('agent-a')]}
        selectedAgentId="agent-a"
        sessions={[]}
        nextCursor="cursor-2"
        loading={true}
        onSelectAgent={vi.fn()}
        onOpenSession={vi.fn()}
        onCreateSession={vi.fn()}
        onLoadNext={onLoadNext}
        onRetry={onRetry}
        onBack={vi.fn()}
      />,
    );
    expect(rendered.lastFrame()).toContain('Loading sessions');

    rendered.rerender(
      <SessionList
        agents={[createAgent('agent-a')]}
        selectedAgentId="agent-a"
        sessions={[]}
        nextCursor="cursor-2"
        error="Could not load sessions"
        onSelectAgent={vi.fn()}
        onOpenSession={vi.fn()}
        onCreateSession={vi.fn()}
        onLoadNext={onLoadNext}
        onRetry={onRetry}
        onBack={vi.fn()}
      />,
    );
    expect(rendered.lastFrame()).toContain('Could not load sessions');
    expect(rendered.lastFrame()).not.toContain('No chat sessions yet');
    await pressInkKey(rendered, 'r');
    expect(onRetry).toHaveBeenCalledTimes(1);

    rendered.rerender(
      <SessionList
        agents={[createAgent('agent-a')]}
        selectedAgentId="agent-a"
        sessions={[]}
        nextCursor="cursor-2"
        onSelectAgent={vi.fn()}
        onOpenSession={vi.fn()}
        onCreateSession={vi.fn()}
        onLoadNext={onLoadNext}
        onBack={vi.fn()}
      />,
    );
    expect(rendered.lastFrame()).toContain('No chat sessions yet');
    await pressInkKey(rendered, 'n');
    expect(onLoadNext).toHaveBeenCalledWith('cursor-2');
  });
});

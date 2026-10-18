import { describe, expect, it } from 'vitest';

import {
  mergeAcpSnapshots,
  formatClock,
  parseCodingSession,
  shortPath,
} from './coding-sessions';

const appResult = {
  kind: 'app_session',
  id: 'app-3',
  command: 'claude',
  cwd: '/Users/x/code/uploader',
  status: 'running',
  exitCode: null,
  startedAt: '2026-10-19T10:00:00Z',
  screen: {
    rows: ['> Fix the flaky test', '  12 passed', '', ''],
    cursor: [2, 0],
    size: [24, 80],
    alternate: true,
  },
  output: 'plain text',
  truncated: false,
  outcome: 'settled',
  containment: 'process_group',
};

const acpResult = {
  kind: 'acp_session',
  id: 'acp-1',
  command: 'codex-acp',
  cwd: '/Users/x/code/uploader',
  agent: { name: 'Codex', version: '0.50.0' },
  sessionId: 'abc',
  status: 'waiting_for_you',
  stopReason: null,
  events: [
    { type: 'agent_message', text: 'Old message.' },
    {
      type: 'plan',
      entries: [
        { content: 'Find where uploads restart', status: 'completed', priority: 'high' },
        { content: 'Keep the byte offset', status: 'in_progress', priority: 'high' },
        { content: '', status: 'pending', priority: 'low' },
      ],
    },
    { type: 'tool_call', id: 't1', title: 'Read src/upload.ts', toolKind: 'read', status: 'pending', locations: [] },
    { type: 'tool_call_update', id: 't1', status: 'completed', text: null },
    { type: 'tool_call', id: 't2', title: 'Edit src/upload.ts', toolKind: 'edit', status: 'pending', locations: [] },
    { type: 'permission_asked', toolCallId: 't2', title: 'Edit src/upload.ts', toolKind: 'edit', locations: [] },
    { type: 'agent_message', text: 'It has a plan ' },
    { type: 'agent_thought', text: 'hmm' },
    { type: 'agent_message', text: 'and wants to edit.' },
  ],
  truncated: false,
};

describe('parseCodingSession', () => {
  it('reads an app session snapshot and drops the unused screen rows', () => {
    expect(parseCodingSession(JSON.stringify(appResult))).toEqual({
      kind: 'app_session',
      id: 'app-3',
      command: 'claude',
      cwd: '/Users/x/code/uploader',
      status: 'running',
      exitCode: null,
      startedAtMs: Date.parse('2026-10-19T10:00:00Z'),
      rows: ['> Fix the flaky test', '  12 passed'],
      output: 'plain text',
      outcome: 'settled',
    });
  });

  it('folds an ACP event log into plan, steps, the latest message and a pending ask', () => {
    const view = parseCodingSession(JSON.stringify(acpResult));
    expect(view).toMatchObject({
      kind: 'acp_session',
      agentName: 'Codex',
      status: 'waiting_for_you',
      plan: [
        { content: 'Find where uploads restart', status: 'completed' },
        { content: 'Keep the byte offset', status: 'in_progress' },
      ],
      steps: [
        { id: 't1', title: 'Read src/upload.ts', status: 'completed', decision: null },
        { id: 't2', title: 'Edit src/upload.ts', status: 'pending', decision: null },
      ],
      message: 'It has a plan and wants to edit.',
      waitingOn: { title: 'Edit src/upload.ts', toolKind: 'edit' },
    });
  });

  it('clears the pending ask once it is answered', () => {
    const answered = {
      ...acpResult,
      status: 'running',
      events: [
        ...acpResult.events,
        { type: 'permission_answered', toolCallId: 't2', decision: 'allowed' },
      ],
    };
    const view = parseCodingSession(JSON.stringify(answered));
    expect(view).toMatchObject({
      waitingOn: null,
      steps: [{ id: 't1' }, { id: 't2', decision: 'allowed' }],
    });
  });

  it.each([
    ['plain text', 'done'],
    ['a result cut short', JSON.stringify(appResult).slice(0, 200)],
    ['another JSON kind', JSON.stringify({ kind: 'app_sessions', sessions: [] })],
    ['an unknown status', JSON.stringify({ ...appResult, status: 'sleeping' })],
    ['no id', JSON.stringify({ ...acpResult, id: '' })],
    ['an array', '[1,2]'],
  ])('returns null for %s', (_label, result) => {
    expect(parseCodingSession(result)).toBeNull();
  });
});

describe('formatting', () => {
  it('shortens home paths and formats clocks', () => {
    expect(shortPath('/Users/x/code/uploader')).toBe('~/code/uploader');
    expect(shortPath('/srv/app')).toBe('/srv/app');
    expect(formatClock(134_000)).toBe('2:14');
    expect(formatClock(3_734_000)).toBe('1:02:14');
  });
});

describe('mergeAcpSnapshots', () => {
  it('keeps the plan, steps and message of earlier reads until newer ones arrive', () => {
    const first = parseCodingSession(
      JSON.stringify({
        kind: 'acp_session',
        id: 'acp-1',
        agent: { name: 'Codex' },
        status: 'running',
        events: [
          { type: 'plan', entries: [{ content: 'Find the bug', status: 'in_progress' }] },
          { type: 'tool_call', id: 't1', title: 'Edit a.ts', toolKind: 'edit', status: 'pending' },
          { type: 'permission_asked', toolCallId: 't1', title: 'Edit a.ts', toolKind: 'edit' },
          { type: 'permission_answered', toolCallId: 't1', decision: 'allowed' },
          { type: 'agent_message', text: 'Working on it' },
        ],
      }),
    );
    const second = parseCodingSession(
      JSON.stringify({
        kind: 'acp_session',
        id: 'acp-1',
        agent: { name: 'Codex' },
        status: 'idle',
        stopReason: 'end_turn',
        events: [{ type: 'tool_call_update', id: 't9', status: 'completed' }],
      }),
    );
    if (first?.kind !== 'acp_session' || second?.kind !== 'acp_session')
      throw new Error('expected ACP snapshots');
    const merged = mergeAcpSnapshots(first, second);
    expect(merged.status).toBe('idle');
    expect(merged.plan).toEqual(first.plan);
    expect(merged.steps).toEqual(first.steps);
    expect(merged.steps[0]?.decision).toBe('allowed');
    expect(merged.message).toBe('Working on it');
  });
});

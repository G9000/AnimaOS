import { describe, expect, it } from 'vitest';
import type { Approval, LearningReview, Run } from '@animaOS-SWARM/sdk';

import type { TelegramConnector } from './daemon-api';
import { approvalHeadline, doneActions, needsYouItems } from './needs-you';
import { parsePlan } from './plan';

function approval(overrides: Partial<Approval> = {}): Approval {
  return {
    id: 'ap-1',
    agentId: 'agent-1',
    sessionId: 'chat:main',
    runId: 'run-1',
    toolCallId: 'call-1',
    tool: 'bash',
    class: 'exec',
    arguments: JSON.stringify({ command: 'git status' }),
    argumentsTruncated: false,
    suggestedMatcher: { kind: 'any', value: '' },
    matcherKinds: ['any'],
    createdAtMs: 100,
    expiresAtMs: 10_000,
    status: 'pending',
    revision: 1,
    resolution: null,
    ...overrides,
  };
}

function telegram(overrides: Partial<TelegramConnector> = {}): TelegramConnector {
  return {
    id: 'tg-1',
    agentId: 'agent-1',
    roomId: 'telegram:tg-1',
    type: 'telegram',
    bot: { id: '42', username: 'anima_julio_bot', displayName: null },
    approvedChat: null,
    pendingPairing: null,
    status: 'ready',
    enabled: true,
    createdAtMs: 1,
    updatedAtMs: 50,
    ...overrides,
  };
}

function review(overrides: Partial<LearningReview> = {}): LearningReview {
  return {
    id: 'rev-1',
    agentId: 'agent-1',
    sourceRunId: 'run-1',
    sessionId: 'chat:main',
    trigger: 'manual',
    policyVersion: 1,
    evidenceDigest: 'x',
    evidenceBrief: '',
    evidenceAnchors: [],
    state: 'completed',
    reservedAtMs: 10,
    finishedAtMs: 300,
    model: 'm',
    usageComplete: true,
    proposals: [
      {
        id: 'p-1',
        observation: 'o',
        suggestion: 'Sort receipts into expense categories',
        anchors: [],
        decision: 'pending',
        decisionVersion: 1,
      },
      {
        id: 'p-2',
        observation: 'o',
        suggestion: 'Already handled',
        anchors: [],
        decision: 'dismissed',
        decisionVersion: 2,
      },
    ],
    ...overrides,
  } as LearningReview;
}

describe('needsYouItems', () => {
  it('puts approvals, pairing requests, broken bots and pending skills in one queue, newest first', () => {
    const items = needsYouItems({
      approvals: [approval()],
      telegram: [
        telegram({
          pendingPairing: {
            chat: { id: '101', type: 'private', title: null, username: 'julio' },
            requestedAtMs: 200,
          },
        } as Partial<TelegramConnector>),
        telegram({ id: 'tg-2', status: 'credentialRequired', updatedAtMs: 150 }),
      ],
      reviews: [review()],
    });
    expect(items.map((item) => item.kind)).toEqual([
      'review',
      'pair',
      'fix',
      'approve',
    ]);
    const pair = items[1];
    expect(pair.kind === 'pair' && pair.chatName).toBe('julio');
    // A dismissed proposal is not asked again.
    expect(items.filter((item) => item.kind === 'review')).toHaveLength(1);
  });

  it('leaves out reviews that have not finished and healthy bots', () => {
    expect(
      needsYouItems({
        approvals: [],
        telegram: [telegram()],
        reviews: [review({ state: 'running' })],
      }),
    ).toEqual([]);
  });
});

describe('approvalHeadline', () => {
  it('says what a call does with its main argument', () => {
    expect(approvalHeadline(approval())).toEqual({
      title: 'Runs commands · bash',
      detail: 'git status',
    });
  });

  it('shows a plan by its title and step count', () => {
    const plan = approval({
      tool: 'propose_plan',
      class: 'write',
      arguments: JSON.stringify({
        title: 'Lisbon trip',
        steps: [{ action: 'Compare flights' }, { action: 'Book', asks_first: true }],
      }),
    });
    expect(approvalHeadline(plan)).toEqual({
      title: 'Plan: Lisbon trip',
      detail: '2 steps · nothing starts until you say so',
    });
  });
});

describe('parsePlan', () => {
  it('reads steps written as text or objects and drops empty ones', () => {
    expect(
      parsePlan(
        JSON.stringify({
          title: ' Trip ',
          steps: ['Compare flights', { action: 'Book it', asks_first: true }, ''],
          estimate: 'about 6 min',
        }),
      ),
    ).toEqual({
      title: 'Trip',
      steps: [
        { action: 'Compare flights', asksFirst: false },
        { action: 'Book it', asksFirst: true },
      ],
      estimate: 'about 6 min',
    });
  });

  it('rejects cut-short or stepless arguments', () => {
    expect(parsePlan('{"title":"Trip","steps":[')).toBeNull();
    expect(parsePlan(JSON.stringify({ title: 'Trip', steps: [] }))).toBeNull();
  });
});

describe('doneActions', () => {
  const run = (receipts: Run['actionReceipts']): Run =>
    ({ id: 'run-1', sessionId: 'chat:main', actionReceipts: receipts }) as Run;
  const receipt = (name: string, args: object, status = 'succeeded', at = 10) => ({
    toolCallId: `${name}-${at}`,
    name,
    status: status as 'succeeded',
    startedAtMs: at,
    finishedAtMs: at + 1,
    durationMs: 1,
    argumentsPreview: JSON.stringify(args),
    argumentsTruncated: false,
    resultPreview: null,
    resultTruncated: false,
    durability: 'persistent' as const,
  });

  it('lists effects in plain words, newest first, and leaves out reads and unfinished calls', () => {
    const actions = doneActions([
      run([
        receipt('read_file', { path: 'notes.md' }),
        receipt('edit_file', { path: 'anima.toml' }, 'succeeded', 20),
        receipt('bash', { command: 'git push' }, 'failed', 30),
        receipt('web_fetch', { url: 'https://example.com' }, 'started', 40),
      ]),
    ]);
    expect(
      actions.map(({ title, detail, failed }) => ({ title, detail, failed })),
    ).toEqual([
      { title: 'Ran a command', detail: 'git push', failed: true },
      { title: 'Changed a file', detail: 'anima.toml', failed: false },
    ]);
  });
});

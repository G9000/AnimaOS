import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { LearningPolicy, LearningReview, Run } from '@animaOS-SWARM/sdk';

import type { AgentDetail } from '../../lib/types';
import { buildTranscript } from '../../lib/transcript';
import { emptyLiveRun } from '../../lib/session-events';
import { onOpenInbox } from '../../lib/inbox-events';
import { nextReviewTime, taskReviewView } from '../../lib/task-reviews';
import { runFixture } from '../../test/live';
import { MessageList } from '../ChatScreen';

function receipts(count: number): NonNullable<Run['actionReceipts']> {
  return Array.from({ length: count }, (_, index) => ({
    toolCallId: `call-${index}`,
    name: 'memory_search',
    status: 'succeeded' as const,
    startedAtMs: 1,
    finishedAtMs: 2,
    durationMs: 1,
    argumentsPreview: '{}',
    argumentsTruncated: false,
    resultPreview: 'ok',
    resultTruncated: false,
    durability: 'persistent' as const,
  }));
}

const run = runFixture('long-task', {
  status: 'completed',
  startedAtMs: 1,
  finishedAtMs: 3,
  toolsStarted: ['memory_search'],
  replyMessageId: 'reply',
  actionReceipts: receipts(3),
});

const messages: AgentDetail['messages'] = [
  {
    id: 'request',
    role: 'User',
    content: { text: 'Find the note.' },
    created_at_ms: 1,
  },
  {
    id: 'reply',
    role: 'Assistant',
    content: { text: 'Found it.', metadata: { runId: run.id } },
    created_at_ms: 3,
  },
];

const agent: AgentDetail = {
  id: 'agent-1',
  name: 'Nova',
  provider: 'openai',
  model: 'gpt-5',
  toolNames: [],
  created_at_ms: 1,
  status: 'Idle',
  token_usage: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 },
  messages,
};

function policy(overrides: Partial<LearningPolicy> = {}): LearningPolicy {
  return {
    mode: 'strict',
    autoEnabled: false,
    dailyLimit: 2,
    pendingLimit: 10,
    timeoutMs: 20_000,
    windowMs: 86_400_000,
    remaining: 2,
    nextAvailableAtMs: null,
    autoMinActions: 5,
    ...overrides,
  };
}

function review(overrides: Partial<LearningReview> = {}): LearningReview {
  return {
    id: 'review-1',
    agentId: agent.id,
    sourceRunId: run.id,
    sessionId: run.sessionId,
    trigger: 'automatic',
    policyVersion: 1,
    evidenceDigest: 'digest',
    evidenceBrief: 'brief',
    evidenceAnchors: ['input'],
    state: 'completed',
    reservedAtMs: 1,
    model: 'gpt-5',
    usageComplete: true,
    proposals: [],
    ...overrides,
  };
}

const lesson = {
  id: 'lesson-1',
  observation: 'The same sum was checked five times.',
  suggestion: 'Check a sum once, then move on.',
  anchors: ['input'],
  decision: 'acknowledged' as const,
  decisionVersion: 1,
  memoryId: 'memory-1',
};

function renderList(
  current: LearningPolicy,
  reviews: LearningReview[] = [],
  overrides: Partial<Record<string, unknown>> = {},
) {
  const api = {
    policy: vi.fn(async () => current),
    listReviews: vi.fn(async () => reviews),
    requestReview: vi.fn(async () =>
      review({ trigger: 'manual', state: 'queued' }),
    ),
    undoLesson: vi.fn(async () =>
      review({
        proposals: [{ ...lesson, decision: 'dismissed', decisionVersion: 2 }],
      }),
    ),
    ...overrides,
  };
  render(
    <MessageList
      agent={agent}
      sending={false}
      scrollerRef={{ current: null }}
      onSuggestion={vi.fn()}
      items={buildTranscript({ messages, runs: [emptyLiveRun(run)] })}
      actions={{ taskReviews: api }}
    />,
  );
  return api;
}

describe('task review line', () => {
  it('offers a review in Strict mode while the budget allows', async () => {
    const user = userEvent.setup();
    const api = renderList(policy({ remaining: 1 }));
    expect(await screen.findByText('1 of 2 left')).toBeVisible();
    // The daemon lists the admitted review from now on.
    api.listReviews.mockResolvedValue([
      review({ trigger: 'manual', state: 'queued' }),
    ]);
    await user.click(screen.getByRole('button', { name: 'Review task' }));
    expect(api.requestReview).toHaveBeenCalledWith(agent.id, run.id);
    expect(await screen.findByText('Looking back at this task…')).toBeVisible();
  });

  it('shows when the next review opens instead of a dead button', async () => {
    const next = new Date(2026, 9, 19, 15, 42).getTime();
    renderList(policy({ remaining: 0, nextAvailableAtMs: next }));
    expect(
      await screen.findByText(/Task reviews are used for now · next one/),
    ).toBeVisible();
    expect(
      screen.queryByRole('button', { name: 'Review task' }),
    ).not.toBeInTheDocument();
  });

  it('turns a budget refusal into the quiet used state', async () => {
    const user = userEvent.setup();
    const used = policy({
      remaining: 0,
      nextAvailableAtMs: Date.now() + 60_000,
    });
    const api = renderList(policy({ remaining: 1 }), [], {
      requestReview: vi.fn(async () => {
        throw new Error('Task reviews are used up for now (2 per 24 hours)');
      }),
    });
    const offer = await screen.findByRole('button', { name: 'Review task' });
    api.policy.mockResolvedValue(used);
    await user.click(offer);
    expect(
      await screen.findByText(/Task reviews are used for now/),
    ).toBeVisible();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('shows nothing in Free mode until a lesson is kept', async () => {
    const api = renderList(policy({ mode: 'free' }));
    await waitFor(() => expect(api.listReviews).toHaveBeenCalled());
    expect(
      screen.queryByRole('button', { name: 'Review task' }),
    ).not.toBeInTheDocument();
    expect(screen.queryByLabelText('Task learning review')).toBeNull();
  });

  it('says what Free mode learned and lets the owner undo it', async () => {
    const user = userEvent.setup();
    const api = renderList(policy({ mode: 'free' }), [
      review({ proposals: [lesson] }),
    ]);
    expect(
      await screen.findByText('Learned: Check a sum once, then move on.'),
    ).toBeVisible();
    await user.click(
      screen.getByRole('button', {
        name: 'Undo lesson: Check a sum once, then move on.',
      }),
    );
    expect(api.undoLesson).toHaveBeenCalledWith(
      agent.id,
      'review-1',
      'lesson-1',
      {
        expectedVersion: 1,
      },
    );
    expect(await screen.findByText('Forgot that lesson.')).toBeVisible();
  });

  it('points a Strict suggestion to the Inbox', async () => {
    const user = userEvent.setup();
    const opened = vi.fn();
    const stop = onOpenInbox(opened);
    renderList(policy(), [
      review({
        proposals: [
          {
            ...lesson,
            decision: 'pending',
            decisionVersion: 0,
            memoryId: null,
          },
        ],
      }),
    ]);
    expect(await screen.findByText('Suggested a lesson')).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Open Inbox' }));
    expect(opened).toHaveBeenCalledTimes(1);
    stop();
  });
});

describe('taskReviewView', () => {
  it('keeps automatic reviews that found nothing silent', () => {
    expect(taskReviewView(review(), policy())).toEqual({ kind: 'none' });
    expect(taskReviewView(review({ state: 'failed' }), policy())).toEqual({
      kind: 'none',
    });
    expect(taskReviewView(review({ trigger: 'manual' }), policy())).toEqual({
      kind: 'nothing',
    });
  });

  it('offers nothing while the policy is unknown', () => {
    expect(taskReviewView(undefined, null)).toEqual({ kind: 'none' });
  });
});

describe('nextReviewTime', () => {
  it('names tomorrow when the next review is after midnight', () => {
    const now = new Date(2026, 9, 19, 22, 0).getTime();
    const later = new Date(2026, 9, 19, 23, 30).getTime();
    const tomorrow = new Date(2026, 9, 20, 9, 5).getTime();
    expect(nextReviewTime(later, now)).not.toMatch(/tomorrow/);
    expect(nextReviewTime(tomorrow, now)).toMatch(/^tomorrow at /);
  });
});

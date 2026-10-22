import React from 'react';
import { Text } from 'ink';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { AgentEvent } from '@animaos/sdk';
import type { TuiReviewApi } from '../session-api.js';
import { approval, clarification } from '../../tests/review-fixtures.js';
import { cleanupInk, flushInk, renderInk } from '../test-harness.js';
import {
  useSessionReview,
  type SessionReview,
  type SessionReviewOptions,
} from './use-session-review.js';

afterEach(() => {
  cleanupInk();
  vi.restoreAllMocks();
});

function api(overrides: Partial<TuiReviewApi> = {}): TuiReviewApi {
  return {
    listPendingApprovals: vi.fn().mockResolvedValue([approval()]),
    decideApproval: vi
      .fn()
      .mockResolvedValue(approval({ status: 'allowed', revision: 2 })),
    listClarifications: vi.fn().mockResolvedValue([clarification()]),
    answerClarification: vi
      .fn()
      .mockResolvedValue(clarification({ status: 'answered', revision: 2 })),
    ...overrides,
  };
}

function harness(
  reviewApi: TuiReviewApi,
  options: Partial<SessionReviewOptions> = {},
) {
  let current!: SessionReview;
  function Probe({
    sessionId = 'session-a',
    online = true,
  }: {
    sessionId?: string;
    online?: boolean;
  }) {
    current = useSessionReview({
      api: reviewApi,
      agentId: 'agent-a',
      sessionId,
      online,
      ...options,
    });
    return (
      <Text>
        {current.approvals.length}:{current.clarifications.length}
      </Text>
    );
  }
  const rendered = renderInk(<Probe />);
  return {
    rendered,
    current: () => current,
    change: (sessionId: string, online = true) =>
      rendered.rerender(<Probe sessionId={sessionId} online={online} />),
  };
}

describe('useSessionReview', () => {
  it('loads pending requests, filters foreign ownership and blocks offline writes', async () => {
    const reviewApi = api({
      listPendingApprovals: vi
        .fn()
        .mockResolvedValue([
          approval(),
          approval({ id: 'other', agentId: 'foreign' }),
        ]),
    });
    const h = harness(reviewApi);
    await vi.waitFor(() => expect(h.current().approvals).toHaveLength(1));
    await vi.waitFor(() => expect(h.current().clarifications).toHaveLength(1));
    h.change('session-a', false);
    await vi.waitFor(() => expect(h.current().canAct).toBe(false));
    expect(
      await h
        .current()
        .decide(approval(), { decision: 'allow_once', revision: 1 }),
    ).toBe(false);
    expect(reviewApi.decideApproval).not.toHaveBeenCalled();
  });

  it('deduplicates writes and does not restore a resolved approval from a late read', async () => {
    let resolveWrite!: (value: ReturnType<typeof approval>) => void;
    const reviewApi = api({
      decideApproval: vi.fn().mockImplementation(
        () =>
          new Promise((resolve) => {
            resolveWrite = resolve;
          }),
      ),
    });
    const h = harness(reviewApi);
    await vi.waitFor(() => expect(h.current().approvals).toHaveLength(1));
    const first = h
      .current()
      .decide(approval(), { decision: 'allow_once', revision: 1 });
    const duplicate = h
      .current()
      .decide(approval(), { decision: 'allow_once', revision: 1 });
    expect(await duplicate).toBe(false);
    resolveWrite(approval({ status: 'allowed', revision: 2 }));
    expect(await first).toBe(true);
    await vi.waitFor(() => expect(h.current().approvals).toHaveLength(0));
    await h.current().refresh();
    expect(h.current().approvals).toHaveLength(0);
    expect(reviewApi.decideApproval).toHaveBeenCalledTimes(1);
  });

  it('applies approval events and ignores old-scope read and write completions', async () => {
    let resolveWrite!: (value: ReturnType<typeof approval>) => void;
    const reviewApi = api({
      decideApproval: vi.fn().mockImplementation(
        () =>
          new Promise((resolve) => {
            resolveWrite = resolve;
          }),
      ),
    });
    const h = harness(reviewApi);
    await vi.waitFor(() => expect(h.current().approvals).toHaveLength(1));
    h.current().onEvent({
      type: 'approval.resolved',
      agentId: 'agent-a',
      seq: 2,
      at: 2,
      approval: approval({ status: 'denied', revision: 2 }),
    } as AgentEvent);
    await vi.waitFor(() => expect(h.current().approvals).toHaveLength(0));
    h.current().onEvent({
      type: 'approval.requested',
      agentId: 'agent-a',
      seq: 3,
      at: 3,
      approval: approval({ revision: 3 }),
    } as AgentEvent);
    await vi.waitFor(() => expect(h.current().approvals).toHaveLength(1));
    const write = h.current().decide(approval({ revision: 3 }), {
      decision: 'allow_once',
      revision: 3,
    });
    h.change('session-b');
    await vi.waitFor(() => expect(h.current().approvals).toHaveLength(0));
    resolveWrite(approval({ status: 'allowed', revision: 4 }));
    await write;
    expect(h.current().approvals).toHaveLength(0);
    expect(h.current().notice).toBeNull();
  });

  it('refreshes conflicts, preserves transient errors and never automatically retries writes', async () => {
    const reviewApi = api({
      answerClarification: vi
        .fn()
        .mockRejectedValue(
          Object.assign(new Error('Changed'), { status: 409 }),
        ),
    });
    const h = harness(reviewApi);
    await vi.waitFor(() => expect(h.current().clarifications).toHaveLength(1));
    expect(
      await h.current().answer(clarification(), { revision: 1, answers: [] }),
    ).toBe(false);
    await vi.waitFor(() => expect(h.current().error).toContain('changed'));
    expect(reviewApi.answerClarification).toHaveBeenCalledTimes(1);
    expect(reviewApi.listClarifications).toHaveBeenCalledTimes(2);
  });

  it('keeps a newer pending revision when subsequent polls return stale versions', async () => {
    const reviewApi = api();
    const h = harness(reviewApi);
    await vi.waitFor(() => expect(h.current().approvals).toHaveLength(1));
    h.current().onEvent({
      type: 'approval.requested',
      agentId: 'agent-a',
      seq: 3,
      at: 3,
      approval: approval({ revision: 3 }),
    } as AgentEvent);
    await vi.waitFor(() => expect(h.current().approvals[0]?.revision).toBe(3));
    await h.current().refresh();
    await flushInk();
    expect(h.current().approvals[0]?.revision).toBe(3);
  });

  it('requires a fresh form after a conflict but permits a new decision after an authoritative refresh', async () => {
    const decide = vi
      .fn()
      .mockRejectedValueOnce(
        Object.assign(new Error('Rule list is full'), { status: 409 }),
      )
      .mockResolvedValue(approval({ status: 'allowed', revision: 2 }));
    const h = harness(api({ decideApproval: decide }));
    await vi.waitFor(() => expect(h.current().approvals).toHaveLength(1));
    const formVersion = h.current().formVersions['a:approval-a'] ?? 0;
    expect(
      await h.current().decide(approval(), {
        decision: 'allow_always',
        revision: 1,
        matcher: approval().suggestedMatcher,
      }),
    ).toBe(false);
    await flushInk();
    expect(h.current().formVersions['a:approval-a']).toBeGreaterThan(
      formVersion,
    );
    expect(h.current().approvalBlocked(approval())).toBe(false);
    expect(
      await h
        .current()
        .decide(approval(), { decision: 'allow_once', revision: 1 }),
    ).toBe(true);
    expect(decide).toHaveBeenCalledTimes(2);
  });

  it('includes only helpers with an exact known run and session association', async () => {
    const helper = approval({
      id: 'helper-a',
      agentId: 'helper',
      runId: 'helper-run',
    });
    const reviewApi = api({
      listPendingApprovals: vi
        .fn()
        .mockResolvedValue([
          approval(),
          helper,
          approval({ id: 'bad-helper', agentId: 'helper', runId: 'other' }),
          approval({ id: 'wrong-session', sessionId: 'session-b' }),
        ]),
      listClarifications: vi.fn().mockImplementation((owner) =>
        Promise.resolve(
          owner === 'helper'
            ? [
                clarification({
                  id: 'helper-q',
                  agentId: 'helper',
                  runId: 'helper-run',
                }),
              ]
            : [clarification()],
        ),
      ),
    });
    const h = harness(reviewApi, {
      runAgentIds: { 'helper-run': 'helper' },
      runSessionIds: { 'helper-run': 'session-a' },
    });
    await vi.waitFor(() => expect(h.current().approvals).toHaveLength(2));
    expect(h.current().approvals.map((item) => item.id)).toEqual([
      'approval-a',
      'helper-a',
    ]);
    await vi.waitFor(() => expect(h.current().clarifications).toHaveLength(2));
    expect(reviewApi.listClarifications).toHaveBeenCalledWith(
      'helper',
      'session-a',
      expect.any(AbortSignal),
    );
  });

  it('aborts reads on navigation and never displays a late response from the old session', async () => {
    let resolveRead!: (items: ReturnType<typeof approval>[]) => void;
    const signals: AbortSignal[] = [];
    const reviewApi = api({
      listPendingApprovals: vi
        .fn()
        .mockImplementationOnce((signal) => {
          signals.push(signal);
          return new Promise((resolve) => {
            resolveRead = resolve;
          });
        })
        .mockResolvedValue([]),
    });
    const h = harness(reviewApi);
    await vi.waitFor(() => expect(signals).toHaveLength(1));
    h.change('session-b');
    await vi.waitFor(() => expect(signals[0].aborted).toBe(true));
    resolveRead([approval()]);
    await vi.waitFor(() =>
      expect(reviewApi.listPendingApprovals).toHaveBeenCalledTimes(2),
    );
    expect(h.current().approvals).toHaveLength(0);
  });

  it('retains same-session requests after a failed read, disables that source, and recovers explicitly', async () => {
    const list = vi
      .fn()
      .mockResolvedValueOnce([approval()])
      .mockRejectedValueOnce(new Error('unreachable'))
      .mockResolvedValue([approval()]);
    const h = harness(api({ listPendingApprovals: list }));
    await vi.waitFor(() => expect(h.current().approvals).toHaveLength(1));
    await h.current().refresh();
    await vi.waitFor(() => expect(h.current().error).toContain('unreachable'));
    expect(h.current().approvals).toHaveLength(1);
    expect(h.current().approvalBlocked(approval())).toBe(true);
    await h.current().refresh();
    await vi.waitFor(() => expect(h.current().error).toBeNull());
    expect(h.current().approvalBlocked(approval())).toBe(false);
  });

  it('polls for clarifications and stops the timer on unmount', async () => {
    const interval = vi.spyOn(globalThis, 'setInterval');
    const clear = vi.spyOn(globalThis, 'clearInterval');
    const reviewApi = api();
    const h = harness(reviewApi);
    await vi.waitFor(() => expect(h.current().clarifications).toHaveLength(1));
    const callIndex = interval.mock.calls.findIndex(
      (call) => call[1] === 2_000,
    );
    expect(callIndex).toBeGreaterThanOrEqual(0);
    const tick = interval.mock.calls[callIndex][0] as () => void;
    tick();
    await vi.waitFor(() =>
      expect(reviewApi.listClarifications).toHaveBeenCalledTimes(2),
    );
    h.rendered.unmount();
    expect(clear).toHaveBeenCalledWith(interval.mock.results[callIndex].value);
  });

  it('keeps cached requests read-only during reconnect until fresh reads finish', async () => {
    let resolveRead!: (items: ReturnType<typeof approval>[]) => void;
    let resolveClarifications!: (
      items: ReturnType<typeof clarification>[],
    ) => void;
    const listA = vi
      .fn()
      .mockResolvedValueOnce([approval()])
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveRead = resolve;
          }),
      );
    const listC = vi
      .fn()
      .mockResolvedValueOnce([clarification()])
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveClarifications = resolve;
          }),
      );
    const h = harness(
      api({ listPendingApprovals: listA, listClarifications: listC }),
    );
    await vi.waitFor(() => expect(h.current().clarifications).toHaveLength(1));
    h.change('session-a', false);
    await vi.waitFor(() => expect(h.current().canAct).toBe(false));
    h.change('session-a', true);
    await vi.waitFor(() => expect(listA).toHaveBeenCalledTimes(2));
    expect(h.current().approvalBlocked(approval())).toBe(true);
    expect(h.current().clarificationBlocked(clarification())).toBe(true);
    resolveRead([approval()]);
    resolveClarifications([clarification()]);
    await vi.waitFor(() =>
      expect(h.current().approvalBlocked(approval())).toBe(false),
    );
  });
});

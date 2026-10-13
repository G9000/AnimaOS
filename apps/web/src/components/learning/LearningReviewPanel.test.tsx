import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { LearningReview } from '@animaOS-SWARM/sdk';
import {
  LearningReviewPanel,
  type LearningReviewApi,
} from './LearningReviewPanel';

function review(overrides: Partial<LearningReview> = {}): LearningReview {
  return {
    id: 'review-1',
    agentId: 'agent-1',
    sourceRunId: 'run-1',
    sessionId: 'chat:1',
    trigger: 'manual',
    policyVersion: 1,
    evidenceDigest: 'digest',
    evidenceBrief: 'The task completed after checking the source.',
    evidenceAnchors: ['run-1'],
    state: 'completed',
    reservedAtMs: 1,
    startedAtMs: 2,
    finishedAtMs: 3,
    model: 'gpt-5.5',
    provider: 'openai',
    usage: null,
    usageComplete: false,
    error: null,
    proposals: [
      {
        id: 'proposal-1',
        observation: 'The result was checked against a source.',
        suggestion: 'Keep a source citation with future summaries.',
        anchors: ['run-1'],
        decision: 'pending',
        decisionVersion: 3,
      },
    ],
    ...overrides,
  };
}

function apiMock(
  overrides: Partial<LearningReviewApi> = {},
): LearningReviewApi {
  return {
    policy: vi.fn(async () => ({
      autoEnabled: false,
      dailyLimit: 2,
      pendingLimit: 10,
      timeoutMs: 20_000,
    })),
    setPolicy: vi.fn(async (_id, input) => ({
      autoEnabled: input.autoEnabled,
      dailyLimit: 2,
      pendingLimit: 10,
      timeoutMs: 20_000,
    })),
    listReviews: vi.fn(async () => []),
    getReview: vi.fn(async () => review()),
    cancelReview: vi.fn(async (_agentId, id) =>
      review({ id, state: 'cancelled' }),
    ),
    decideProposal: vi.fn(async (_agentId, reviewId, proposalId, input) =>
      review({
        id: reviewId,
        proposals: [
          {
            ...review().proposals[0]!,
            id: proposalId,
            decision: input.decision,
            decisionVersion: input.expectedVersion + 1,
          },
        ],
      }),
    ),
    ...overrides,
  };
}

describe('LearningReviewPanel', () => {
  it('persists the optional automatic-review policy separately from settings save', async () => {
    const user = userEvent.setup();
    const api = apiMock();
    render(<LearningReviewPanel agentId="agent-1" api={api} />);

    const optIn = await screen.findByRole('checkbox', {
      name: 'Automatically review completed tasks',
    });
    await waitFor(() => expect(optIn).toBeEnabled());
    expect(optIn).not.toBeChecked();
    await user.click(optIn);

    await waitFor(() =>
      expect(api.setPolicy).toHaveBeenCalledWith('agent-1', {
        autoEnabled: true,
      }),
    );
    expect(optIn).toBeChecked();
    expect(api.policy).toHaveBeenCalledWith('agent-1', expect.anything());
  });

  it('reads, cancels and resolves suggestions with the exact proposal version', async () => {
    const user = userEvent.setup();
    const queued = review({
      id: 'review-running',
      state: 'running',
      proposals: [],
    });
    const completed = review({ id: 'review-complete' });
    const api = apiMock({
      listReviews: vi.fn(async () => [queued, completed]),
      getReview: vi.fn(async () => completed),
      cancelReview: vi.fn(async (_agentId, id) =>
        review({ id, state: 'cancelled', proposals: [] }),
      ),
    });
    render(<LearningReviewPanel agentId="agent-1" api={api} />);

    await screen.findByText('running · 0 suggestions');
    const completedCard = screen
      .getByText('completed · 1 suggestion')
      .closest('article');
    expect(completedCard).not.toBeNull();
    await user.click(
      within(completedCard!).getByRole('button', { name: 'Read review' }),
    );
    expect(
      await screen.findByText('The task completed after checking the source.'),
    ).toBeVisible();
    expect(
      screen.getByText(
        'Unverified advice · not a saved fact or approved skill',
      ),
    ).toBeVisible();
    expect(api.getReview).toHaveBeenCalledWith('agent-1', 'review-complete');

    const runningStatus = screen
      .getByText('running · 0 suggestions')
      .closest('article');
    await user.click(
      within(runningStatus!).getByRole('button', { name: 'Cancel review' }),
    );
    await waitFor(() =>
      expect(api.cancelReview).toHaveBeenCalledWith(
        'agent-1',
        'review-running',
      ),
    );
    expect(await screen.findByText('cancelled · 0 suggestions')).toBeVisible();

    const acknowledge = await screen.findByRole('button', {
      name: 'Acknowledge',
    });
    await user.click(acknowledge);
    expect(api.decideProposal).toHaveBeenCalledWith(
      'agent-1',
      'review-complete',
      'proposal-1',
      {
        decision: 'acknowledged',
        expectedVersion: 3,
      },
    );
    await waitFor(() => expect(screen.getByText('acknowledged')).toBeVisible());
  });

  it('refreshes a review after a stale proposal decision and leaves it available to inspect', async () => {
    const user = userEvent.setup();
    const current = review({
      proposals: [
        {
          ...review().proposals[0]!,
          decisionVersion: 4,
        },
      ],
    });
    const api = apiMock({
      listReviews: vi
        .fn()
        .mockResolvedValueOnce([review()])
        .mockResolvedValue([current]),
      decideProposal: vi.fn(async () => {
        throw new Error('Proposal changed; review it again');
      }),
    });
    render(<LearningReviewPanel agentId="agent-1" api={api} />);

    await user.click(
      await screen.findByRole('button', { name: 'Read review' }),
    );
    await user.click(await screen.findByRole('button', { name: 'Dismiss' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Proposal changed',
    );
    await waitFor(() => expect(api.listReviews).toHaveBeenCalledTimes(2));
    expect(screen.getByRole('button', { name: 'Dismiss' })).toBeVisible();
  });

  it('refreshes review history only when explicitly requested', async () => {
    const user = userEvent.setup();
    const api = apiMock({
      listReviews: vi
        .fn()
        .mockResolvedValueOnce([])
        .mockResolvedValueOnce([review()]),
    });
    render(<LearningReviewPanel agentId="agent-1" api={api} />);
    await screen.findByText('No task reviews yet.');
    expect(api.listReviews).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole('button', { name: 'Refresh reviews' }));
    expect(await screen.findByText('completed · 1 suggestion')).toBeVisible();
    expect(api.listReviews).toHaveBeenCalledTimes(2);
  });

  it('serializes detail reads against cancel decisions so an older read cannot win', async () => {
    const user = userEvent.setup();
    let resolveDetail!: (value: LearningReview) => void;
    const queued = review({
      id: 'review-running',
      state: 'running',
      proposals: [],
    });
    const api = apiMock({
      listReviews: vi.fn(async () => [queued]),
      getReview: vi.fn(
        () =>
          new Promise((resolve) => {
            resolveDetail = resolve;
          }),
      ),
      cancelReview: vi.fn(async (_agentId, id) =>
        review({ id, state: 'cancelled', proposals: [] }),
      ),
    });
    render(<LearningReviewPanel agentId="agent-1" api={api} />);

    const card = (await screen.findByText('running · 0 suggestions')).closest(
      'article',
    );
    await user.click(
      within(card!).getByRole('button', { name: 'Read review' }),
    );
    const cancel = within(card!).getByRole('button', { name: 'Cancel review' });
    expect(cancel).toBeDisabled();

    resolveDetail(queued);
    await waitFor(() => expect(cancel).toBeEnabled());
    await user.click(cancel);
    expect(await screen.findByText('cancelled · 0 suggestions')).toBeVisible();
  });

  it('ignores a delayed policy response from the previous agent', async () => {
    let resolveOld!: (
      value: Awaited<ReturnType<LearningReviewApi['policy']>>,
    ) => void;
    const api = apiMock({
      policy: vi.fn((agentId) =>
        agentId === 'agent-old'
          ? new Promise((resolve) => {
              resolveOld = resolve;
            })
          : Promise.resolve({
              autoEnabled: true,
              dailyLimit: 2,
              pendingLimit: 10,
              timeoutMs: 20_000,
            }),
      ),
    });
    const view = render(<LearningReviewPanel agentId="agent-old" api={api} />);
    view.rerender(<LearningReviewPanel agentId="agent-new" api={api} />);
    const checkbox = await screen.findByRole('checkbox', {
      name: 'Automatically review completed tasks',
    });
    await waitFor(() => expect(checkbox).toBeChecked());

    resolveOld({
      autoEnabled: false,
      dailyLimit: 2,
      pendingLimit: 10,
      timeoutMs: 20_000,
    });
    await waitFor(() => expect(checkbox).toBeChecked());
  });

  it('lets a new agent change policy while an old agent save is still pending', async () => {
    const user = userEvent.setup();
    let resolveOldWrite!: (
      value: Awaited<ReturnType<LearningReviewApi['setPolicy']>>,
    ) => void;
    const api = apiMock({
      setPolicy: vi.fn((agentId, input) =>
        agentId === 'agent-old'
          ? new Promise((resolve) => {
              resolveOldWrite = resolve;
            })
          : Promise.resolve({
              autoEnabled: input.autoEnabled,
              dailyLimit: 2,
              pendingLimit: 10,
              timeoutMs: 20_000,
            }),
      ),
    });
    const view = render(<LearningReviewPanel agentId="agent-old" api={api} />);
    const checkbox = await screen.findByRole('checkbox', {
      name: 'Automatically review completed tasks',
    });
    await waitFor(() => expect(checkbox).toBeEnabled());
    await user.click(checkbox);
    await waitFor(() =>
      expect(api.setPolicy).toHaveBeenCalledWith('agent-old', {
        autoEnabled: true,
      }),
    );

    view.rerender(<LearningReviewPanel agentId="agent-new" api={api} />);
    await waitFor(() => expect(checkbox).toBeEnabled());
    await user.click(checkbox);
    await waitFor(() =>
      expect(api.setPolicy).toHaveBeenCalledWith('agent-new', {
        autoEnabled: true,
      }),
    );
    expect(checkbox).toBeChecked();

    resolveOldWrite({
      autoEnabled: false,
      dailyLimit: 2,
      pendingLimit: 10,
      timeoutMs: 20_000,
    });
    await waitFor(() => expect(checkbox).toBeChecked());
  });
});

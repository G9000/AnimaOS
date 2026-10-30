import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { DaemonHttpError } from '@animaos/sdk';
import type {
  LearningReview,
  LearningSkillDraftInput,
} from '@animaos/sdk';
import { skillDraftFixture } from '../../test/skills';
import { skillFixture } from '../../test/skills';
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
    listSkills: vi.fn(async () => []),
    skill: vi.fn(async () => ({ skill: null, file: null })),
    createSkillDraft: vi.fn(async () => {
      throw new Error('No skill draft fixture');
    }),
    ...overrides,
  };
}

describe('LearningReviewPanel', () => {
  it('M39_offers_explicit_skill_draft_handoff_from_a_read_review', async () => {
    const user = userEvent.setup();
    const completed = review({ id: 'review-for-draft' });
    const api = apiMock({
      listReviews: vi.fn(async () => [completed]),
      getReview: vi.fn(async () => completed),
    });
    render(<LearningReviewPanel agentId="agent-1" api={api} />);

    const card = (await screen.findByText('completed · 1 suggestion')).closest(
      'article',
    );
    await user.click(
      within(card!).getByRole('button', { name: 'Read review' }),
    );
    expect(
      await screen.findByRole('button', { name: 'Create skill draft' }),
    ).toBeVisible();
  });

  it('creates only a pending draft with the exact edited bytes, source digest, version, and selected base hash', async () => {
    const user = userEvent.setup();
    const completed = review({ id: 'review-for-draft' });
    const existing = skillFixture('source-checking', {
      name: 'Source checking',
      description: 'Check cited sources.',
      approvedHash: 'approved-hash',
    });
    const draft = skillDraftFixture('skd-learning', {
      slug: existing.slug,
      name: 'Source checking',
      description: 'Check cited sources.',
      body: 'Verify the cited source before summarizing.',
      learningSource: {
        agentId: 'agent-1',
        reviewId: completed.id,
        proposalId: completed.proposals[0]!.id,
        sourceRunId: completed.sourceRunId,
        sessionId: completed.sessionId,
        evidenceDigest: completed.evidenceDigest,
        decisionVersion: 3,
      },
    });
    const createSkillDraft = vi.fn(
      async (
        _agentId: string,
        _reviewId: string,
        _proposalId: string,
        _input: LearningSkillDraftInput,
      ) => draft,
    );
    const api = apiMock({
      listReviews: vi.fn(async () => [completed]),
      getReview: vi.fn(async () => completed),
      listSkills: vi.fn(async () => [existing]),
      skill: vi.fn(async () => ({
        skill: existing,
        file: {
          hash: 'current-file-hash',
          name: existing.name,
          description: existing.description,
          body: 'Existing instructions.',
          problem: null,
        },
      })),
      createSkillDraft,
    });
    const onOpenSkills = vi.fn();
    render(
      <LearningReviewPanel
        agentId="agent-1"
        api={api}
        onOpenSkills={onOpenSkills}
      />,
    );
    const card = (await screen.findByText('completed · 1 suggestion')).closest(
      'article',
    );
    await user.click(
      within(card!).getByRole('button', { name: 'Read review' }),
    );
    await user.click(
      await screen.findByRole('button', { name: 'Create skill draft' }),
    );
    const base = await screen.findByLabelText('Base skill');
    await waitFor(() => expect(base).toBeEnabled());
    await user.selectOptions(base, 'source-checking');

    const body = await screen.findByRole('textbox', {
      name: 'Skill instructions',
    });
    await waitFor(() =>
      expect(body).toHaveValue(
        'Existing instructions.\n\nKeep a source citation with future summaries.',
      ),
    );
    await user.clear(body);
    await user.type(body, 'Verify the cited source before summarizing.');
    await user.click(
      screen.getByRole('button', { name: 'Create pending skill draft' }),
    );

    await waitFor(() =>
      expect(createSkillDraft).toHaveBeenCalledWith(
        'agent-1',
        'review-for-draft',
        'proposal-1',
        {
          expectedVersion: 3,
          evidenceDigest: 'digest',
          skill: {
            slug: 'source-checking',
            name: 'Source checking',
            description: 'Check cited sources.',
            body: 'Verify the cited source before summarizing.',
            expectedHash: 'current-file-hash',
          },
        },
      ),
    );
    expect(
      await screen.findByText(/Pending skill draft created/),
    ).toBeVisible();
    expect(screen.getByText(/separate review and approval/)).toBeVisible();
    await user.click(
      screen.getByRole('button', { name: 'Open skill draft in Skills' }),
    );
    expect(onOpenSkills).toHaveBeenCalledOnce();
  });

  it('keeps every edited skill field visible when the source or base hash conflicts', async () => {
    const user = userEvent.setup();
    const api = apiMock({
      listReviews: vi.fn(async () => [review()]),
      createSkillDraft: vi.fn(async () => {
        throw new DaemonHttpError(409, { error: 'Source evidence changed' });
      }),
    });
    render(<LearningReviewPanel agentId="agent-1" api={api} />);
    await user.click(
      await screen.findByRole('button', { name: 'Read review' }),
    );
    await user.click(
      await screen.findByRole('button', { name: 'Create skill draft' }),
    );
    const slug = await screen.findByRole('textbox', { name: 'Skill slug' });
    const name = screen.getByRole('textbox', { name: 'Skill name' });
    const description = screen.getByRole('textbox', {
      name: 'Skill description',
    });
    const body = screen.getByRole('textbox', { name: 'Skill instructions' });
    await user.clear(slug);
    await user.type(slug, 'reviewed-source-check');
    await user.clear(name);
    await user.type(name, 'Reviewed source check');
    await user.clear(description);
    await user.type(description, 'Check source material.');
    await user.clear(body);
    await user.type(body, 'Compare the cited source.');
    await user.click(
      screen.getByRole('button', { name: 'Create pending skill draft' }),
    );

    expect(await screen.findByRole('alert')).toHaveTextContent(
      /edits are kept/,
    );
    expect(slug).toHaveValue('reviewed-source-check');
    expect(name).toHaveValue('Reviewed source check');
    expect(description).toHaveValue('Check source material.');
    expect(body).toHaveValue('Compare the cited source.');
  });

  it('clears a pending base-skill read when switching back to a new skill', async () => {
    const user = userEvent.setup();
    const existing = skillFixture('slow-skill');
    let resolveSkill!: (
      value: Awaited<ReturnType<LearningReviewApi['skill']>>,
    ) => void;
    const api = apiMock({
      listReviews: vi.fn(async () => [review()]),
      listSkills: vi.fn(async () => [existing]),
      skill: vi.fn(
        () =>
          new Promise((resolve) => {
            resolveSkill = resolve;
          }),
      ),
    });
    render(<LearningReviewPanel agentId="agent-1" api={api} />);
    await user.click(
      await screen.findByRole('button', { name: 'Read review' }),
    );
    await user.click(
      await screen.findByRole('button', { name: 'Create skill draft' }),
    );
    const base = await screen.findByLabelText('Base skill');
    await waitFor(() => expect(base).toBeEnabled());
    await user.selectOptions(base, 'slow-skill');
    expect(await screen.findByText('Loading selected skill…')).toBeVisible();

    await user.selectOptions(base, '');
    expect(
      screen.queryByText('Loading selected skill…'),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Create pending skill draft' }),
    ).toBeEnabled();

    resolveSkill({
      skill: existing,
      file: {
        hash: 'late-hash',
        name: existing.name,
        description: existing.description,
        body: 'late body',
        problem: null,
      },
    });
    expect(
      await screen.findByRole('textbox', { name: 'Skill instructions' }),
    ).toHaveValue('Keep a source citation with future summaries.');
    expect(
      screen.queryByText(/Based on the selected skill version/),
    ).not.toBeInTheDocument();
  });

  it('does not attach a delayed old-agent skill draft to a new agent review', async () => {
    const user = userEvent.setup();
    let resolveDraft!: (value: ReturnType<typeof skillDraftFixture>) => void;
    const oldReview = review({ agentId: 'agent-old', id: 'same-review' });
    const newReview = review({ agentId: 'agent-new', id: 'same-review' });
    const api = apiMock({
      listReviews: vi.fn(async (agentId) => [
        agentId === 'agent-old' ? oldReview : newReview,
      ]),
      getReview: vi.fn(async (agentId) =>
        agentId === 'agent-old' ? oldReview : newReview,
      ),
      createSkillDraft: vi.fn(
        () =>
          new Promise((resolve) => {
            resolveDraft = resolve;
          }),
      ),
    });
    const view = render(<LearningReviewPanel agentId="agent-old" api={api} />);
    await user.click(
      await screen.findByRole('button', { name: 'Read review' }),
    );
    await user.click(
      await screen.findByRole('button', { name: 'Create skill draft' }),
    );
    await user.click(
      screen.getByRole('button', { name: 'Create pending skill draft' }),
    );
    await waitFor(() => expect(api.createSkillDraft).toHaveBeenCalledOnce());
    expect(screen.getByRole('button', { name: 'Acknowledge' })).toBeDisabled();

    view.rerender(<LearningReviewPanel agentId="agent-new" api={api} />);
    const newAgentCard = (
      await screen.findByText('completed · 1 suggestion')
    ).closest('article');
    await user.click(
      within(newAgentCard!).getByRole('button', { name: 'Read review' }),
    );
    await screen.findByRole('button', { name: 'Create skill draft' });
    resolveDraft(skillDraftFixture('old-agent-draft'));
    await waitFor(() =>
      expect(
        screen.getByRole('button', { name: 'Create skill draft' }),
      ).toBeVisible(),
    );
    expect(
      screen.queryByText(/A skill draft is linked to this advice/),
    ).not.toBeInTheDocument();
  });

  it('persists the optional automatic-review policy separately from settings save', async () => {
    const user = userEvent.setup();
    const api = apiMock();
    render(<LearningReviewPanel agentId="agent-1" api={api} />);

    const optIn = await screen.findByRole('switch', {
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
    const checkbox = await screen.findByRole('switch', {
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
    const checkbox = await screen.findByRole('switch', {
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

  it('renders each core-memory edit of a conversation review and accepts or rejects it', async () => {
    const user = userEvent.setup();
    const proposal = review().proposals[0]!;
    const conversation = review({
      id: 'review-conversation',
      trigger: 'conversation',
      proposals: [
        {
          ...proposal,
          id: 'edit-add',
          observation: 'The owner asked for kilometres.',
          coreEdit: {
            action: 'add',
            block: 'user',
            text: 'Prefers metric units',
          },
        },
        {
          ...proposal,
          id: 'edit-replace',
          observation: 'The owner corrected the indentation.',
          coreEdit: {
            action: 'replace',
            block: 'notes',
            oldText: 'Indent with tabs',
            text: 'Indent with four spaces',
          },
        },
        {
          ...proposal,
          id: 'edit-remove',
          observation: 'The owner moved.',
          decision: 'acknowledged',
          memoryId: 'mem-1',
          coreEdit: {
            action: 'remove',
            block: 'user',
            oldText: 'Lives in Paris',
          },
        },
        {
          ...proposal,
          id: 'edit-failed',
          observation: 'Too much.',
          decision: 'failed',
          coreEdit: {
            action: 'add',
            block: 'notes',
            text: 'x',
            error:
              'Agent notes is full (2,500/2,500 chars). Replace or remove an entry first.',
          },
        },
      ],
    });
    const api = apiMock({
      listReviews: vi.fn(async () => [conversation]),
      getReview: vi.fn(async () => conversation),
      decideProposal: vi.fn(async (_agentId, _reviewId, proposalId, input) => ({
        ...conversation,
        proposals: conversation.proposals.map((item) =>
          item.id === proposalId
            ? {
                ...item,
                decision: input.decision,
                decisionVersion: input.expectedVersion + 1,
              }
            : item,
        ),
      })),
    });
    render(<LearningReviewPanel agentId="agent-1" api={api} />);

    expect(await screen.findByText('Conversation review')).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Read review' }));
    expect(
      await screen.findByText('Add to About you: Prefers metric units'),
    ).toBeVisible();
    expect(
      screen.getByText(
        'Replace in Agent notes: Indent with tabs → Indent with four spaces',
      ),
    ).toBeVisible();
    expect(
      screen.getByText('Remove from About you: Lives in Paris'),
    ).toBeVisible();
    expect(screen.getByText('Applied to core memory')).toBeVisible();
    expect(
      screen.getByText(
        'Not applied: Agent notes is full (2,500/2,500 chars). Replace or remove an entry first.',
      ),
    ).toBeVisible();
    // A core edit is not advice and never becomes a skill draft.
    expect(
      screen.queryByRole('button', { name: 'Create skill draft' }),
    ).toBeNull();

    const [accept] = screen.getAllByRole('button', { name: 'Accept' });
    await user.click(accept!);
    expect(api.decideProposal).toHaveBeenCalledWith(
      'agent-1',
      'review-conversation',
      'edit-add',
      { decision: 'acknowledged', expectedVersion: 3 },
    );
    await waitFor(() =>
      expect(screen.getAllByText('Applied to core memory')).toHaveLength(2),
    );
    await user.click(screen.getByRole('button', { name: 'Reject' }));
    expect(api.decideProposal).toHaveBeenCalledWith(
      'agent-1',
      'review-conversation',
      'edit-replace',
      { decision: 'dismissed', expectedVersion: 3 },
    );
    expect(await screen.findByText('Rejected')).toBeVisible();
  });
});

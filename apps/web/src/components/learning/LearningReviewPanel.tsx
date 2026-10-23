import { useEffect, useRef, useState } from 'react';
import type {
  LearningCoreEdit,
  LearningDecision,
  LearningPolicy,
  LearningProposal,
  LearningReview,
  LearningSkillDraftInput,
  Skill,
  SkillDetail,
  SkillDraft,
} from '@animaos/sdk';
import { SectionTitle } from '../ui-bits';
import { LearningSkillDraftForm } from './LearningSkillDraftForm';
import { useLatest } from '../../hooks/useLatest';
import { useResetOnChange } from '../../hooks/useResetOnChange';

export interface LearningReviewApi {
  policy(
    agentId: string,
    options?: { signal?: AbortSignal },
  ): Promise<LearningPolicy>;
  setPolicy(
    agentId: string,
    input: { autoEnabled: boolean },
  ): Promise<LearningPolicy>;
  listReviews(
    agentId: string,
    options?: { signal?: AbortSignal },
  ): Promise<LearningReview[]>;
  getReview(
    agentId: string,
    reviewId: string,
    options?: { signal?: AbortSignal },
  ): Promise<LearningReview>;
  cancelReview(agentId: string, reviewId: string): Promise<LearningReview>;
  decideProposal(
    agentId: string,
    reviewId: string,
    proposalId: string,
    input: { decision: LearningDecision; expectedVersion: number },
  ): Promise<LearningReview>;
  listSkills(options?: { signal?: AbortSignal }): Promise<Skill[]>;
  skill(slug: string, options?: { signal?: AbortSignal }): Promise<SkillDetail>;
  createSkillDraft(
    agentId: string,
    reviewId: string,
    proposalId: string,
    input: LearningSkillDraftInput,
  ): Promise<SkillDraft>;
}

export function LearningReviewPanel({
  agentId,
  api,
  disabled = false,
  onOpenSkills,
}: {
  agentId: string;
  api?: LearningReviewApi;
  disabled?: boolean;
  onOpenSkills?: () => void;
}) {
  const [policy, setPolicy] = useState<LearningPolicy | null>(null);
  const [reviews, setReviews] = useState<LearningReview[]>([]);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(
    () => new Set(),
  );
  const [busy, setBusy] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(api !== undefined);
  const generation = useRef(0);
  const busyRef = useRef(false);
  const currentAgent = useLatest(agentId);
  useResetOnChange([agentId, api], () => {
    setPolicy(null);
    setReviews([]);
    setExpanded(new Set());
    setBusy(false);
    setRefreshing(false);
    setError(null);
    setLoading(api !== undefined);
  });
  useEffect(() => {
    const requestGeneration = ++generation.current;
    let active = true;
    const controller = new AbortController();
    busyRef.current = false;
    if (!api) {
      return () => {
        active = false;
      };
    }
    void Promise.all([
      api.policy(agentId, { signal: controller.signal }),
      api.listReviews(agentId, { signal: controller.signal }),
    ])
      .then(([nextPolicy, nextReviews]) => {
        if (!active || generation.current !== requestGeneration) return;
        setPolicy(nextPolicy);
        setReviews(nextReviews);
        setLoading(false);
      })
      .catch((cause: unknown) => {
        if (!active || generation.current !== requestGeneration) return;
        setError(errorMessage(cause));
        setLoading(false);
      });
    return () => {
      active = false;
      controller.abort();
      if (generation.current === requestGeneration) generation.current += 1;
    };
  }, [agentId, api]);

  const refreshReviews = async (
    expectedGeneration = generation.current,
    acquireBusy = false,
  ) => {
    if (!api) return;
    if (acquireBusy) {
      if (busyRef.current) return;
      busyRef.current = true;
      setBusy(true);
      setRefreshing(true);
    }
    try {
      const next = await api.listReviews(agentId);
      if (
        generation.current === expectedGeneration &&
        currentAgent.current === agentId
      )
        setReviews(next);
    } catch (cause) {
      if (generation.current === expectedGeneration)
        setError(errorMessage(cause));
    } finally {
      if (acquireBusy && generation.current === expectedGeneration) {
        busyRef.current = false;
        setBusy(false);
        setRefreshing(false);
      }
    }
  };

  const changePolicy = async (autoEnabled: boolean) => {
    if (!api || !policy || busyRef.current || disabled) return;
    const requestGeneration = generation.current;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      const next = await api.setPolicy(agentId, { autoEnabled });
      if (
        generation.current === requestGeneration &&
        currentAgent.current === agentId
      )
        setPolicy(next);
    } catch (cause) {
      if (generation.current === requestGeneration)
        setError(errorMessage(cause));
    } finally {
      if (generation.current === requestGeneration) {
        busyRef.current = false;
        setBusy(false);
      }
    }
  };

  const cancel = async (review: LearningReview) => {
    if (!api || busyRef.current || disabled) return;
    const requestGeneration = generation.current;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      const next = await api.cancelReview(agentId, review.id);
      if (
        generation.current === requestGeneration &&
        currentAgent.current === agentId
      )
        replaceReview(next);
    } catch (cause) {
      if (generation.current === requestGeneration)
        setError(errorMessage(cause));
      await refreshReviews(requestGeneration);
    } finally {
      if (generation.current === requestGeneration) {
        busyRef.current = false;
        setBusy(false);
      }
    }
  };

  const decide = async (
    review: LearningReview,
    proposalId: string,
    decision: LearningDecision,
  ) => {
    if (!api || busyRef.current || disabled) return;
    const proposal = review.proposals.find((item) => item.id === proposalId);
    if (!proposal) return;
    const requestGeneration = generation.current;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      const next = await api.decideProposal(agentId, review.id, proposalId, {
        decision,
        expectedVersion: proposal.decisionVersion,
      });
      if (
        generation.current === requestGeneration &&
        currentAgent.current === agentId
      )
        replaceReview(next);
    } catch (cause) {
      if (generation.current === requestGeneration)
        setError(errorMessage(cause));
      await refreshReviews(requestGeneration);
    } finally {
      if (generation.current === requestGeneration) {
        busyRef.current = false;
        setBusy(false);
      }
    }
  };

  const readReview = async (reviewId: string) => {
    if (!api || busyRef.current) return;
    const nextExpanded = new Set(expanded);
    if (nextExpanded.has(reviewId)) {
      nextExpanded.delete(reviewId);
      setExpanded(nextExpanded);
      return;
    }
    nextExpanded.add(reviewId);
    setExpanded(nextExpanded);
    const requestGeneration = generation.current;
    busyRef.current = true;
    setBusy(true);
    try {
      const latest = await api.getReview(agentId, reviewId);
      if (
        generation.current === requestGeneration &&
        currentAgent.current === agentId
      )
        replaceReview(latest);
    } catch (cause) {
      if (generation.current === requestGeneration)
        setError(errorMessage(cause));
    } finally {
      if (generation.current === requestGeneration) {
        busyRef.current = false;
        setBusy(false);
      }
    }
  };

  function replaceReview(next: LearningReview) {
    setReviews((current) => {
      const index = current.findIndex((item) => item.id === next.id);
      if (index < 0) return [next, ...current];
      const updated = [...current];
      updated[index] = next;
      return updated;
    });
  }

  const panelGeneration = generation.current;

  return (
    <section aria-labelledby="learning-review-heading" className="space-y-3">
      <SectionTitle>Learning review</SectionTitle>
      <h4 id="learning-review-heading" className="sr-only">
        Learning review
      </h4>
      <div className="rounded-xl border border-line bg-white/[0.015] p-3.5">
        <div className="flex items-start gap-3">
          <input
            id="learning-auto-review"
            type="checkbox"
            aria-describedby="learning-auto-review-help"
            checked={policy?.autoEnabled ?? false}
            disabled={disabled || busy || loading || !api || policy === null}
            onChange={(event) => void changePolicy(event.target.checked)}
            className="mt-0.5 h-4 w-4 shrink-0"
          />
          <label
            htmlFor="learning-auto-review"
            className="cursor-pointer font-medium text-ink"
          >
            Automatically review completed tasks
          </label>
        </div>
        <p
          id="learning-auto-review-help"
          className="mt-1 pl-7 text-sm text-ink-2"
        >
          Reviews produce suggestions for you to inspect. They do not change
          saved facts or skills.
        </p>
        {loading ? (
          <p role="status" className="mt-2 pl-7 text-xs text-ink-3">
            Loading learning reviews…
          </p>
        ) : null}
      </div>

      {error ? (
        <div
          role="alert"
          className="rounded-lg border border-danger/30 bg-danger/[0.05] p-3 text-sm text-danger"
        >
          {error}
        </div>
      ) : null}

      <div className="space-y-2" aria-label="Learning review history">
        <div className="flex justify-end">
          <button
            type="button"
            className="studio-tool-button"
            disabled={!api || loading || busy}
            onClick={() => void refreshReviews(generation.current, true)}
          >
            {refreshing ? 'Refreshing reviews…' : 'Refresh reviews'}
          </button>
        </div>
        {reviews.length === 0 && !loading ? (
          <p className="text-sm text-ink-3">No task reviews yet.</p>
        ) : null}
        {reviews.map((review) => (
          <article
            key={review.id}
            className="rounded-xl border border-line bg-white/[0.015] p-3.5"
          >
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <p className="font-medium text-ink">
                  {review.trigger === 'conversation'
                    ? 'Conversation review'
                    : 'Task review'}
                </p>
                <p className="mt-1 text-xs text-ink-3">
                  {review.state} · {review.proposals.length} suggestion
                  {review.proposals.length === 1 ? '' : 's'}
                </p>
              </div>
              <div className="flex shrink-0 gap-2">
                <button
                  type="button"
                  className="studio-tool-button"
                  aria-expanded={expanded.has(review.id)}
                  onClick={() => void readReview(review.id)}
                  disabled={busy}
                >
                  {expanded.has(review.id) ? 'Hide review' : 'Read review'}
                </button>
                {review.state === 'queued' || review.state === 'running' ? (
                  <button
                    type="button"
                    className="studio-tool-button"
                    onClick={() => void cancel(review)}
                    disabled={disabled || busy}
                  >
                    Cancel review
                  </button>
                ) : null}
              </div>
            </div>
            {expanded.has(review.id) ? (
              <div className="mt-3 space-y-3 border-t border-line pt-3">
                <div>
                  <p className="text-xs font-semibold uppercase tracking-wider text-ink-3">
                    Evidence
                  </p>
                  <p className="mt-1 whitespace-pre-wrap break-words text-sm text-ink-2">
                    {review.evidenceBrief}
                  </p>
                  <p className="mt-2 text-xs text-ink-3">
                    Usage:{' '}
                    {review.usage
                      ? `${review.usage.totalTokens} tokens (${review.usage.promptTokens} prompt, ${review.usage.completionTokens} completion)`
                      : 'not reported'}
                    {review.usageComplete ? '' : ' · incomplete'}
                  </p>
                </div>
                {review.error ? (
                  <p role="status" className="text-sm text-ink-2">
                    {review.error.message}
                  </p>
                ) : null}
                {review.proposals.map((proposal) =>
                  proposal.coreEdit ? (
                    <CoreEditProposal
                      key={proposal.id}
                      proposal={proposal}
                      edit={proposal.coreEdit}
                      disabled={disabled || busy}
                      onDecide={(decision) =>
                        void decide(review, proposal.id, decision)
                      }
                    />
                  ) : review.trigger === 'conversation' ? (
                    // A conversation's lessons are not tied to one task, so
                    // they cannot become a skill draft.
                    <div
                      key={proposal.id}
                      className="rounded-lg border border-line p-3"
                    >
                      <LessonBody
                        proposal={proposal}
                        disabled={disabled || busy}
                        onDecide={(decision) =>
                          void decide(review, proposal.id, decision)
                        }
                      />
                    </div>
                  ) : (
                    <LearningSkillDraftForm
                      key={proposal.id}
                      agentId={agentId}
                      review={review}
                      proposal={proposal}
                      api={api}
                      disabled={disabled || busy}
                      onCreated={(draft) => {
                        if (
                          generation.current !== panelGeneration ||
                          currentAgent.current !== agentId
                        )
                          return;
                        setReviews((current) =>
                          current.map((item) =>
                            item.id !== review.id
                              ? item
                              : {
                                  ...item,
                                  proposals: item.proposals.map(
                                    (currentProposal) =>
                                      currentProposal.id === proposal.id
                                        ? {
                                            ...currentProposal,
                                            skillDraftId: draft.id,
                                          }
                                        : currentProposal,
                                  ),
                                },
                          ),
                        );
                      }}
                      onSavingChange={(saving) => {
                        if (generation.current !== panelGeneration)
                          return false;
                        if (saving) {
                          if (busyRef.current) return false;
                          busyRef.current = true;
                          setBusy(true);
                          return true;
                        }
                        busyRef.current = false;
                        setBusy(false);
                        return true;
                      }}
                      onOpenSkills={onOpenSkills}
                    >
                      <LessonBody
                        proposal={proposal}
                        disabled={disabled || busy}
                        onDecide={(decision) =>
                          void decide(review, proposal.id, decision)
                        }
                      />
                    </LearningSkillDraftForm>
                  ),
                )}
              </div>
            ) : null}
          </article>
        ))}
      </div>
    </section>
  );
}

function LessonBody({
  proposal,
  disabled,
  onDecide,
}: {
  proposal: LearningProposal;
  disabled: boolean;
  onDecide: (decision: LearningDecision) => void;
}) {
  return (
    <>
      <p className="text-xs font-semibold uppercase tracking-wider text-amber-300">
        Unverified advice · not a saved fact or approved skill
      </p>
      <p className="text-xs font-semibold uppercase tracking-wider text-ink-3">
        Observation
      </p>
      <p className="mt-1 whitespace-pre-wrap break-words text-sm text-ink">
        {proposal.observation}
      </p>
      <p className="mt-2 text-xs font-semibold uppercase tracking-wider text-ink-3">
        Suggestion
      </p>
      <p className="mt-1 whitespace-pre-wrap break-words text-sm text-ink-2">
        {proposal.suggestion}
      </p>
      <p className="mt-2 text-xs text-ink-3">
        Evidence: {proposal.anchors.join(', ') || 'task record'}
      </p>
      {proposal.decision === 'pending' ? (
        <div className="mt-3 flex gap-2">
          <button
            type="button"
            className="studio-tool-button"
            disabled={disabled}
            onClick={() => onDecide('acknowledged')}
          >
            Acknowledge
          </button>
          <button
            type="button"
            className="studio-tool-button"
            disabled={disabled}
            onClick={() => onDecide('dismissed')}
          >
            Dismiss
          </button>
        </div>
      ) : (
        <p className="mt-3 text-xs text-ink-3">{proposal.decision}</p>
      )}
    </>
  );
}

const CORE_BLOCK_LABEL: Record<LearningCoreEdit['block'], string> = {
  user: 'About you',
  notes: 'Agent notes',
};

/** "Add to About you: …", "Replace in Agent notes: … → …", "Remove from …". */
function coreEditLine(edit: LearningCoreEdit): string {
  const label = CORE_BLOCK_LABEL[edit.block];
  switch (edit.action) {
    case 'add':
      return `Add to ${label}: ${edit.text ?? ''}`;
    case 'replace':
      return `Replace in ${label}: ${edit.oldText ?? ''} → ${edit.text ?? ''}`;
    case 'remove':
      return `Remove from ${label}: ${edit.oldText ?? ''}`;
  }
}

const CORE_DECISION_LABEL: Record<
  Exclude<LearningProposal['decision'], 'pending'>,
  string
> = {
  acknowledged: 'Applied to core memory',
  dismissed: 'Rejected',
  failed: 'Not applied',
};

/** A core-memory edit from a conversation review: accepting applies it. */
function CoreEditProposal({
  proposal,
  edit,
  disabled,
  onDecide,
}: {
  proposal: LearningProposal;
  edit: LearningCoreEdit;
  disabled: boolean;
  onDecide: (decision: LearningDecision) => void;
}) {
  return (
    <div className="rounded-lg border border-line p-3">
      <p className="text-xs font-semibold uppercase tracking-wider text-ink-3">
        Core memory edit
      </p>
      <p className="mt-1 whitespace-pre-wrap break-words text-sm text-ink">
        {coreEditLine(edit)}
      </p>
      <p className="mt-1 whitespace-pre-wrap break-words text-sm text-ink-2">
        {proposal.observation}
      </p>
      {proposal.decision === 'pending' ? (
        <div className="mt-3 flex gap-2">
          <button
            type="button"
            className="studio-tool-button"
            disabled={disabled}
            onClick={() => onDecide('acknowledged')}
          >
            Accept
          </button>
          <button
            type="button"
            className="studio-tool-button"
            disabled={disabled}
            onClick={() => onDecide('dismissed')}
          >
            Reject
          </button>
        </div>
      ) : (
        <p className="mt-3 text-xs text-ink-3">
          {CORE_DECISION_LABEL[proposal.decision]}
          {proposal.decision === 'failed' && edit.error
            ? `: ${edit.error}`
            : ''}
        </p>
      )}
    </div>
  );
}

function errorMessage(cause: unknown): string {
  return cause instanceof Error
    ? cause.message
    : 'Learning review request failed.';
}

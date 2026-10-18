import { useEffect, useState } from 'react';
import type { LearningProposal, LearningReview, Run } from '@animaOS-SWARM/sdk';

import { openInbox } from '../../lib/inbox-events';
import {
  autoReviews,
  nextReviewTime,
  taskReviewView,
} from '../../lib/task-reviews';
import { AlertIcon, ClockIcon, InboxIcon, SparkIcon } from '../icons';
import { useTaskReviews, type TaskReviews } from './TaskReviewContext';

const quiet = 'ml-auto flex max-w-2xl items-center gap-2 text-xs text-ink-3';
const link =
  'inline-flex items-center gap-1 font-medium text-ink-2 hover:text-ink hover:underline disabled:opacity-60';

/** The review line under a finished task: what looking back found, kept
 *  lessons with Undo (Free), suggestions waiting in the Inbox (Strict), or
 *  an offer to look back while the budget allows. */
export function TaskReviewAction({ run }: { run: Run }) {
  const reviews = useTaskReviews();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const review = reviews?.reviewFor(run.id);
  const policy = reviews?.policy ?? null;
  const auto = autoReviews(run, policy);

  // An automatic review is admitted just after its task; look for it.
  useEffect(() => {
    if (auto && !review) reviews?.refreshSoon();
  }, [auto, review, reviews]);

  if (!reviews) return null;
  const view = taskReviewView(review, policy);

  const request = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    setError(await reviews.request(run));
    setBusy(false);
  };

  switch (view.kind) {
    case 'none':
      return null;
    case 'reviewing':
      return (
        <p role="status" aria-label="Task learning review" className={quiet}>
          <SparkIcon size={13} />
          Looking back at this task…
        </p>
      );
    case 'learned':
      return (
        <div
          role="group"
          aria-label="Task learning review"
          className="ml-auto flex max-w-2xl flex-col items-end gap-1"
        >
          {view.lessons.map((lesson) => (
            <KeptLesson
              key={lesson.id}
              review={review as LearningReview}
              lesson={lesson}
              reviews={reviews}
            />
          ))}
        </div>
      );
    case 'suggested':
      return (
        <p role="status" aria-label="Task learning review" className={quiet}>
          <SparkIcon size={13} />
          <span className="text-ink-2">
            Suggested {view.count === 1 ? 'a lesson' : `${view.count} lessons`}
          </span>
          · waits for you
          <button type="button" className={link} onClick={openInbox}>
            <InboxIcon size={12} />
            Open Inbox
          </button>
        </p>
      );
    case 'nothing':
      return (
        <p role="status" aria-label="Task learning review" className={quiet}>
          Looked back · nothing worth keeping this time
        </p>
      );
    case 'unfinished':
      return (
        <p role="status" aria-label="Task learning review" className={quiet}>
          <AlertIcon size={13} />
          The review didn’t finish: {view.reason}.
        </p>
      );
    case 'used':
      return (
        <p role="status" aria-label="Task learning review" className={quiet}>
          <ClockIcon size={13} />
          Task reviews are used for now
          {view.nextAtMs !== null
            ? ` · next one ${nextReviewTime(view.nextAtMs)}`
            : ''}
        </p>
      );
    case 'offer':
      return (
        <section
          aria-label="Task learning review"
          className="ml-auto max-w-2xl"
        >
          <div className="flex flex-wrap items-center gap-x-3 gap-y-2 rounded-xl border border-line bg-white/[0.02] px-3.5 py-2.5">
            <p className="min-w-0 flex-1 text-sm text-ink-2">
              Look back at this task for a lesson worth keeping?
            </p>
            {view.remaining !== null && view.limit !== null ? (
              <span className="text-xs text-ink-3">
                {view.remaining} of {view.limit} left
              </span>
            ) : null}
            <button
              type="button"
              className="studio-tool-button"
              onClick={() => void request()}
              disabled={busy}
            >
              {busy ? 'Starting review…' : 'Review task'}
            </button>
          </div>
          {error ? (
            <p role="alert" className="mt-2 text-sm text-danger">
              {error}
            </p>
          ) : null}
        </section>
      );
  }
}

/** "Learned: … · Undo", then "Forgot that lesson." once undone. */
function KeptLesson({
  review,
  lesson,
  reviews,
}: {
  review: LearningReview;
  lesson: LearningProposal;
  reviews: TaskReviews;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (lesson.decision === 'dismissed')
    return <p className={quiet}>Forgot that lesson.</p>;
  const undo = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    setError(await reviews.undo(review, lesson));
    setBusy(false);
  };
  return (
    <>
      <p className={quiet}>
        <SparkIcon size={13} />
        <span className="text-ink-2">Learned: {lesson.suggestion}</span>
        <button
          type="button"
          className={link}
          onClick={() => void undo()}
          disabled={busy}
          aria-label={`Undo lesson: ${lesson.suggestion}`}
        >
          {busy ? 'Undoing…' : 'Undo'}
        </button>
      </p>
      {error ? (
        <p role="alert" className="text-xs text-danger">
          {error}
        </p>
      ) : null}
    </>
  );
}

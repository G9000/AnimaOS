import type { ChatReview } from '@animaos/sdk';

const STATUS: Record<ChatReview['status'], string> = {
  running: 'Reviewing…',
  completed: 'Done',
  failed: 'Failed',
  cancelled: 'Cancelled',
  unavailable: 'Reviewer unavailable',
  interrupted: 'Interrupted',
};

/** Asks another model to check this reply; hidden while a review of it
 *  runs (its note under the reply shows that instead). */
export function SecondOpinionAction({
  messageId,
  review,
  busy,
  onRequest,
}: {
  messageId: string;
  review: ChatReview | null;
  busy: boolean;
  onRequest(messageId: string): void;
}) {
  if (review?.status === 'running') return null;
  return (
    <button
      type="button"
      className="studio-read-aloud"
      disabled={busy}
      onClick={() => onRequest(messageId)}
    >
      {review ? 'Ask again' : 'Second opinion'}
    </button>
  );
}

/** Another model's opinion of the reply above it. It cannot approve or run
 *  anything; the owner reads it as advice. */
export function SecondOpinionNote({
  review,
  onCancel,
}: {
  review: ChatReview;
  onCancel(reviewId: string): void;
}) {
  return (
    <section
      className="second-opinion"
      aria-label="Second opinion"
      data-status={review.status}
    >
      <header>
        <strong>Second opinion</strong>
        {review.reviewer && (
          <span className="second-opinion-model">
            {review.reviewer.provider} / {review.reviewer.model}
          </span>
        )}
        <span className="second-opinion-status">{STATUS[review.status]}</span>
        {review.status === 'running' && (
          <button
            type="button"
            className="studio-read-aloud"
            onClick={() => onCancel(review.id)}
          >
            Cancel
          </button>
        )}
      </header>
      {review.findings && (
        <p className="second-opinion-findings">{review.findings}</p>
      )}
      {review.error && <p role="status">{review.error}</p>}
      {review.evidenceLimited && review.status === 'completed' && (
        <p className="second-opinion-note">
          Based on part of the conversation; the rest did not fit.
        </p>
      )}
    </section>
  );
}

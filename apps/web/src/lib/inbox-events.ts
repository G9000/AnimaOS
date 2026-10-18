/** Page-wide signals between the chat and the header's Inbox, which live in
 *  separate trees. */
const OPEN = 'anima:open-inbox';
const REVIEWS_CHANGED = 'anima:learning-reviews-changed';

function listen(name: string, handler: () => void): () => void {
  window.addEventListener(name, handler);
  return () => window.removeEventListener(name, handler);
}

export const openInbox = () => window.dispatchEvent(new Event(OPEN));
export const onOpenInbox = (handler: () => void) => listen(OPEN, handler);

/** A review settled, so the Inbox's suggestions are stale. */
export const learningReviewsChanged = () =>
  window.dispatchEvent(new Event(REVIEWS_CHANGED));
export const onLearningReviewsChanged = (handler: () => void) =>
  listen(REVIEWS_CHANGED, handler);

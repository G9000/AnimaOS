import { useEffect, useRef, useState } from 'react';
import type { ChatModels, ChatReview } from '@animaos/sdk';

type Api = Pick<ChatModels, 'reviews' | 'review' | 'cancelReview'>;
const labels = { running: 'Review running', completed: 'Review completed', failed: 'Review failed', cancelled: 'Review cancelled', unavailable: 'Reviewer unavailable', interrupted: 'Review interrupted' };

export function ChatReviewPanel({ agentId, sessionId, api, focus = '', clearGeneration }: {
  agentId: string; sessionId: string; api: Api; focus?: string; clearGeneration?: number;
}) {
  const [reviews, setReviews] = useState<ChatReview[] | null>(null);
  const [draftFocus, setDraftFocus] = useState(focus);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const scope = `${agentId}\0${sessionId}\0${clearGeneration ?? ''}`;
  const current = useRef(scope); current.current = scope;
  const attempt = useRef<{ focus: string; requestId: string } | null>(null);
  useEffect(() => setDraftFocus(focus), [focus]);
  useEffect(() => {
    let live = true; const controller = new AbortController();
    setReviews(null); setError(null); setBusy(false); attempt.current = null;
    void api.reviews(agentId, sessionId, { signal: controller.signal }).then((rows) => { if (live) setReviews(rows); }, (caught: unknown) => { if (live) setError(caught instanceof Error ? caught.message : String(caught)); });
    return () => { live = false; controller.abort(); };
  }, [agentId, sessionId, api, clearGeneration]);
  const running = reviews?.some((row) => row.status === 'running') ?? false;
  useEffect(() => {
    if (!running) return;
    let live = true; let pending = false; const controller = new AbortController();
    const timer = setInterval(() => {
      if (pending) return; pending = true;
      void api.reviews(agentId, sessionId, { signal: controller.signal }).then((rows) => { if (live) { setReviews(rows); setError(null); } }, (caught: unknown) => { if (live) setError(caught instanceof Error ? caught.message : String(caught)); }).finally(() => { pending = false; });
    }, 1000);
    return () => { live = false; clearInterval(timer); controller.abort(); };
  }, [agentId, sessionId, api, running, clearGeneration]);

  const request = async () => {
    if (busy || running) return;
    setBusy(true); setError(null);
    const text = draftFocus.trim();
    if (!attempt.current || attempt.current.focus !== text) attempt.current = { focus: text, requestId: crypto.randomUUID() };
    try {
      const row = await api.review(agentId, sessionId, { requestId: attempt.current.requestId, ...(text ? { focus: text } : {}), ...(clearGeneration !== undefined ? { clearGeneration } : {}) });
      if (current.current === scope) { setReviews((rows) => [...(rows ?? []).filter((old) => old.id !== row.id), row]); attempt.current = null; }
    } catch (caught) { if (current.current === scope) setError(caught instanceof Error ? caught.message : String(caught)); }
    finally { if (current.current === scope) setBusy(false); }
  };
  const cancel = async (id: string) => {
    if (busy) return; setBusy(true); setError(null);
    try {
      const row = await api.cancelReview(agentId, sessionId, id);
      if (current.current === scope) setReviews((rows) => (rows ?? []).map((old) => old.id === id ? row : old));
    } catch (caught) { if (current.current === scope) setError(caught instanceof Error ? caught.message : String(caught)); }
    finally { if (current.current === scope) setBusy(false); }
  };
  return <section aria-label="Independent review">
    <p>Another model checks the latest saved answer using supplied chat evidence. Its opinion cannot approve or run actions.</p>
    <label>Review focus (optional)<input className="field" aria-label="Review focus" maxLength={500} value={draftFocus} disabled={busy || running} onChange={(event) => setDraftFocus(event.target.value)} /></label>
    <button type="button" disabled={busy || running || reviews === null} onClick={() => void request()}>{busy ? 'Working…' : 'Review latest answer'}</button>
    {reviews === null && !error && <p role="status">Loading reviews…</p>}
    {reviews?.slice().reverse().map((row) => <article key={row.id} aria-label={labels[row.status]}>
      <p><strong>{labels[row.status]}</strong>{row.reviewer ? ` · ${row.reviewer.provider} / ${row.reviewer.model}` : ''}</p>
      {row.status === 'running' && <button type="button" disabled={busy} onClick={() => void cancel(row.id)}>Cancel review</button>}
      {row.error && <p role="status">{row.error}</p>}
      {row.findings && <p style={{ whiteSpace: 'pre-wrap' }}>{row.findings}</p>}
    </article>)}
    {error && <p role="alert">{error}</p>}
  </section>;
}

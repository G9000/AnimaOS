import { useEffect, useRef, useState } from 'react';
import type { Approval } from '@animaos/sdk';
import { useAttachmentPreview } from '../../hooks/useAttachmentPreview';
import { revealHiddenCharacters, type ApprovalDecide } from '../../lib/approvals';

interface ImageReview {
  prompt: string;
  sourceAttachmentId: string | null;
  sourceName: string | null;
  sourceSha256: string | null;
}
function review(approval: Approval): ImageReview | null {
  if (approval.argumentsTruncated || new TextEncoder().encode(approval.arguments).length > 16 * 1024) return null;
  try {
    const value: unknown = JSON.parse(approval.arguments);
    if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
    const item = value as Record<string, unknown>;
    const keys = ['_anima_exact_image', 'prompt', 'sourceAttachmentId', 'sourceName', 'sourceSha256', 'provider', 'model', 'size', 'quality', 'count', 'disclosure'];
    if (Object.keys(item).length !== keys.length || !keys.every((key) => Object.hasOwn(item, key)) || item['_anima_exact_image'] !== true ||
      typeof item.prompt !== 'string' || !item.prompt.trim() || item.prompt.includes('\0') || new TextEncoder().encode(item.prompt).length > 2048 ||
      item.provider !== 'openai' || item.model !== 'gpt-image-2.5-sunburst' || item.size !== '1024x1024' || item.quality !== 'medium' || item.count !== 1 ||
      typeof item.disclosure !== 'string' || !item.disclosure.trim() || item.disclosure.length > 2048) return null;
    if (approval.tool === 'generate_image') {
      if (item.sourceAttachmentId !== null || item.sourceName !== null || item.sourceSha256 !== null) return null;
    } else if (approval.tool !== 'edit_image' || typeof item.sourceAttachmentId !== 'string' || !/^[A-Za-z0-9][A-Za-z0-9_:-]{0,127}$/.test(item.sourceAttachmentId) ||
      typeof item.sourceName !== 'string' || !item.sourceName || new TextEncoder().encode(item.sourceName).length > 1024 ||
      typeof item.sourceSha256 !== 'string' || !/^[a-f0-9]{64}$/.test(item.sourceSha256)) return null;
    return item as unknown as ImageReview;
  } catch { return null; }
}

export function ImageApprovalCard(props: { approval: Approval; onDecide?: ApprovalDecide }) {
  const { approval } = props;
  return <ImageApprovalInner key={JSON.stringify([approval.id, approval.revision, approval.agentId, approval.sessionId, approval.arguments, approval.argumentsTruncated, approval.status, approval.expiresAtMs, Boolean(props.onDecide)])} {...props} />;
}

function ImageApprovalInner({ approval, onDecide }: { approval: Approval; onDecide?: ApprovalDecide }) {
  const selected = review(approval);
  const edit = approval.tool === 'edit_image';
  const preview = useAttachmentPreview(approval.agentId, selected?.sourceAttachmentId ?? undefined, 'image/png', Boolean(onDecide && selected?.sourceAttachmentId));
  const [busy, setBusy] = useState(false);
  const [settled, setSettled] = useState(false);
  const [uncertain, setUncertain] = useState(false);
  const [expired, setExpired] = useState(() => Date.now() >= approval.expiresAtMs);
  const alive = useRef(false);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  useEffect(() => {
    const delay = Math.min(Math.max(0, approval.expiresAtMs - Date.now()), 2_147_483_647);
    const timer = window.setTimeout(() => setExpired(Date.now() >= approval.expiresAtMs), delay);
    return () => window.clearTimeout(timer);
  }, [approval.expiresAtMs]);
  const blocked = !onDecide || busy || settled || uncertain || approval.status !== 'pending';
  const decide = async (decision: 'allow_once' | 'deny') => {
    if (blocked || (decision === 'allow_once' && (!selected || expired || Date.now() >= approval.expiresAtMs || (edit && !preview.url)))) return;
    setBusy(true);
    try {
      const failure = await onDecide!(approval, { decision });
      if (!alive.current) return;
      if (failure) setUncertain(true); else setSettled(true);
    } catch { if (alive.current) setUncertain(true); }
    finally { if (alive.current) setBusy(false); }
  };
  return <section className="approval-card" aria-label={`Approval needed: ${approval.tool}`}>
    <header className="approval-card-header"><strong>{edit ? 'Edit selected image' : 'Generate image'}</strong></header>
    <p className="approval-card-lead">{edit ? 'Your prompt and the selected image will be sent to OpenAI. API charges apply. The original is preserved.' : 'Your prompt will be sent to OpenAI. API charges apply.'}</p>
    <p className="approval-card-note">One PNG · 1024 × 1024 · Medium quality · gpt-image-2.5-sunburst. Cost is unknown; the provider may charge for this request.</p>
    {selected && <pre className="approval-card-arguments" aria-label="Image prompt">{revealHiddenCharacters(selected.prompt)}</pre>}
    {selected?.sourceAttachmentId && <div aria-label="Selected input image">
      <p>{revealHiddenCharacters(selected.sourceName!)}</p>
      {preview.url && <img src={preview.url} alt={revealHiddenCharacters(selected.sourceName!)} onError={preview.imageFailed} className="max-h-64 max-w-full object-contain" />}
      {preview.loading && <p role="status">Loading selected image…</p>}
      {preview.error && <p role="alert">The selected image could not be previewed. <button type="button" className="studio-tool-button" onClick={preview.retry}>Retry preview</button></p>}
    </div>}
    {!selected && <p role="alert">This image review is incomplete or unavailable.</p>}
    {!onDecide && <p>Image approval is available to the owner.</p>}
    {expired && <p role="status">This approval expired. Ask again in chat.</p>}
    {settled ? <p role="status">Decision sent. Check the chat for the result.</p> : <div className="approval-card-actions">
      <button type="button" className="studio-tool-button" disabled={blocked || !selected || expired || (edit && !preview.url)} onClick={() => void decide('allow_once')}>{edit ? 'Allow once and edit' : 'Allow once and generate'}</button>
      <button type="button" className="studio-tool-button approval-card-deny" disabled={blocked} onClick={() => void decide('deny')}>Deny</button>
    </div>}
    {uncertain && <p role="alert">The decision could not be confirmed. Refresh the chat to check its status.</p>}
  </section>;
}

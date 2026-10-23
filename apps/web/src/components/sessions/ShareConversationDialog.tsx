import { useEffect, useRef, useState } from 'react';
import type { ConversationShareCreated, ConversationShareGrant } from '@animaos/sdk';
import { chatSharesApi } from '../../lib/daemon-api';
import './conversation-sharing.css';

export function shareConversationUrl(shareId: string): string {
  const url = new URL(window.location.href);
  url.search = '';
  url.hash = `/shared/${encodeURIComponent(shareId)}`;
  return url.toString();
}

export function ShareConversationDialog(props: { agentId: string; sessionId: string; title: string; clearGeneration: number; onClose: () => void }) {
  return <ShareAccess key={`${props.agentId}:${props.sessionId}:${props.clearGeneration}`} {...props} />;
}

function ShareAccess({ agentId, sessionId, title, clearGeneration, onClose }: { agentId: string; sessionId: string; title: string; clearGeneration: number; onClose: () => void }) {
  const [grants, setGrants] = useState<ConversationShareGrant[] | null>(null);
  const [viewerName, setViewerName] = useState('');
  const [created, setCreated] = useState<ConversationShareCreated | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const live = useRef(true);
  useEffect(() => {
    live.current = true;
    const controller = new AbortController();
    chatSharesApi.list(agentId, sessionId, controller.signal)
      .then((result) => { if (live.current) setGrants(result.grants); })
      .catch((failure: unknown) => { if (live.current && !controller.signal.aborted) setError(failure instanceof Error ? failure.message : 'Viewer access could not be loaded.'); });
    return () => { live.current = false; controller.abort(); };
  }, [agentId, sessionId]);

  const issue = async () => {
    if (busy || !viewerName.trim()) return;
    setBusy(true); setError(null); setCreated(null);
    try {
      const result = await chatSharesApi.create(agentId, sessionId, { viewerName: viewerName.trim(), clearGeneration });
      if (!live.current) return;
      setCreated(result); setViewerName(''); setGrants((known) => [...(known ?? []), result.grant]);
    } catch (failure) { if (live.current) setError(failure instanceof Error ? failure.message : 'Viewer access could not be created.'); }
    finally { if (live.current) setBusy(false); }
  };

  const revoke = async (shareId: string) => {
    if (busy) return;
    setBusy(true); setError(null);
    try {
      const result = await chatSharesApi.revoke(agentId, sessionId, shareId);
      if (!live.current) return;
      setGrants(result.grants); if (created?.grant.id === shareId) setCreated(null);
    } catch (failure) { if (live.current) setError(failure instanceof Error ? failure.message : 'Viewer access could not be revoked.'); }
    finally { if (live.current) setBusy(false); }
  };

  const copy = async (value: string) => {
    try { await navigator.clipboard.writeText(value); }
    catch { if (live.current) setError('Copy is unavailable. Select and copy the displayed value.'); }
  };

  return <div className="share-dialog-layer"><section className="share-dialog" role="dialog" aria-modal="true" aria-labelledby="share-title">
    <header><h2 id="share-title">Share conversation</h2><button type="button" className="studio-tool-button" aria-label="Close sharing" onClick={onClose}>Close</button></header>
    <p>Give a viewer read-only access to “{title}”. Only user and assistant text is shared; attachments, tool output, and private activity are omitted.</p>
    <p>Access includes future messages in this chat until you revoke it or clear the chat. The viewer’s browser must reach this Anima installation.</p>
    <form onSubmit={(event) => { event.preventDefault(); void issue(); }}>
      <label>Viewer name<input value={viewerName} maxLength={80} onChange={(event) => setViewerName(event.target.value)} placeholder="Name for this viewer" required /></label>
      <button type="submit" className="studio-button-primary" disabled={busy || !viewerName.trim()}>Create viewer access</button>
    </form>
    {created && <section className="share-new-credential" aria-label="New viewer access">
      <strong>Copy the secret now. It is shown only once.</strong>
      <label>Share link<input readOnly value={shareConversationUrl(created.grant.id)} /></label>
      <button type="button" className="studio-tool-button" onClick={() => void copy(shareConversationUrl(created.grant.id))}>Copy share link</button>
      <label>Viewer ID<input readOnly value={created.grant.viewerId} /></label>
      <label>Viewer secret<input readOnly value={created.secret} /></label>
      <button type="button" className="studio-tool-button" onClick={() => void copy(`Viewer ID: ${created.grant.viewerId}\nViewer secret: ${created.secret}`)}>Copy viewer credentials</button>
      <p>The link contains no secret. Give the viewer their ID and secret separately.</p>
    </section>}
    <section aria-label="Conversation viewers"><h3>Viewers</h3>
      {grants === null && !error && <p>Loading viewers…</p>}
      {grants?.length === 0 && <p>No viewers have access.</p>}
      {grants?.map((grant) => <div className="share-viewer" key={grant.id}>
        <span><strong>{grant.viewerName}</strong><small>{grant.active ? 'Read-only access' : 'Access ended'}</small></span>
        {grant.active && <><button type="button" className="studio-tool-button" onClick={() => void copy(shareConversationUrl(grant.id))}>Copy link for {grant.viewerName}</button><button type="button" className="studio-tool-button" disabled={busy} onClick={() => void revoke(grant.id)}>Revoke {grant.viewerName}</button></>}
      </div>)}
    </section>
    {error && <p role="alert">{error}</p>}
  </section></div>;
}

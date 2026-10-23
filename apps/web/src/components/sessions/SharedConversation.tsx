import { useEffect, useRef, useState } from 'react';
import {
  SharedConversationClient,
  type SharedConversation as SharedConversationInfo,
  type SharedConversationPage,
} from '@animaos/sdk';
import './conversation-sharing.css';
import { useResetOnChange } from '../../hooks/useResetOnChange';

/** This surface must mount before any owner console bootstrap or SDK calls. */
export function SharedConversation({
  shareId,
  baseUrl = '',
}: {
  shareId: string;
  baseUrl?: string;
}) {
  return (
    <ViewerAccess
      key={`${baseUrl}:${shareId}`}
      shareId={shareId}
      baseUrl={baseUrl}
    />
  );
}

function ViewerAccess({
  shareId,
  baseUrl,
}: {
  shareId: string;
  baseUrl: string;
}) {
  const [viewerId, setViewerId] = useState('');
  const [secret, setSecret] = useState('');
  const [client, setClient] = useState<SharedConversationClient | null>(null);
  const [info, setInfo] = useState<SharedConversationInfo | null>(null);
  const [page, setPage] = useState<SharedConversationPage | null>(null);
  const [refresh, setRefresh] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);
  useResetOnChange([client, shareId, refresh], () => {
    if (!client) return;
    setBusy(true);
    setPage(null);
    setInfo(null);
    setError(null);
  });

  useEffect(() => {
    if (!client) return;
    const current = ++generation.current;
    const abort = new AbortController();
    Promise.all([
      client.get(shareId, abort.signal),
      client.messages(shareId, { signal: abort.signal }),
    ])
      .then(([selected, messages]) => {
        if (current !== generation.current) return;
        setInfo(selected);
        setPage(messages);
      })
      .catch((failure: unknown) => {
        if (current === generation.current && !abort.signal.aborted)
          setError(
            failure instanceof Error
              ? failure.message
              : 'The shared conversation could not be loaded.',
          );
      })
      .finally(() => {
        if (current === generation.current) setBusy(false);
      });
    return () => {
      generation.current += 1;
      abort.abort();
    };
  }, [client, shareId, refresh]);

  const signOut = () => {
    generation.current += 1;
    setClient(null);
    setPage(null);
    setInfo(null);
    setSecret('');
    setViewerId('');
    setError(null);
    setBusy(false);
  };

  const loadEarlier = async () => {
    if (!client || !page?.nextBefore || busy) return;
    const current = generation.current;
    setBusy(true);
    setError(null);
    try {
      const earlier = await client.messages(shareId, {
        before: page.nextBefore,
      });
      if (current !== generation.current) return;
      setPage((known) =>
        known
          ? {
              ...earlier,
              messages: [
                ...earlier.messages,
                ...known.messages.filter(
                  (message) =>
                    !earlier.messages.some((old) => old.id === message.id),
                ),
              ],
            }
          : earlier,
      );
    } catch (failure) {
      if (current !== generation.current) return;
      setInfo(null);
      setPage(null);
      setError(
        failure instanceof Error
          ? failure.message
          : 'The shared conversation could not be loaded.',
      );
    } finally {
      if (current === generation.current) setBusy(false);
    }
  };

  return (
    <main className="shared-conversation">
      <header>
        <span className="share-eyebrow">Anima · shared conversation</span>
        <h1>{info?.title ?? 'Shared conversation'}</h1>
      </header>
      <p className="share-scope">
        Read-only access to one conversation. Attachments, tool output, and
        private activity are omitted.
      </p>
      {!client ? (
        <form
          className="share-login"
          onSubmit={(event) => {
            event.preventDefault();
            if (!viewerId.trim() || !secret.trim()) return;
            setClient(
              new SharedConversationClient({
                baseUrl,
                viewerId: viewerId.trim(),
                secret: secret.trim(),
              }),
            );
            setSecret('');
          }}
        >
          <p>
            Enter the viewer ID and secret the owner gave you separately from
            this link.
          </p>
          <label>
            Viewer ID
            <input
              value={viewerId}
              onChange={(event) => setViewerId(event.target.value)}
              autoComplete="off"
              required
            />
          </label>
          <label>
            Viewer secret
            <input
              type="password"
              value={secret}
              onChange={(event) => setSecret(event.target.value)}
              autoComplete="off"
              required
            />
          </label>
          <button
            type="submit"
            className="studio-button-primary"
            disabled={!viewerId.trim() || !secret.trim()}
          >
            Open conversation
          </button>
        </form>
      ) : (
        <>
          <div className="share-viewer-controls">
            <span>
              {info ? `Viewing as ${info.viewerName}` : 'Viewer access'}
            </span>
            <button
              type="button"
              className="studio-tool-button"
              disabled={busy}
              onClick={() => setRefresh((value) => value + 1)}
              aria-label="Refresh conversation"
            >
              Refresh
            </button>
            <button
              type="button"
              className="studio-tool-button"
              onClick={signOut}
            >
              Sign out
            </button>
          </div>
          {page?.nextBefore && (
            <button
              type="button"
              className="studio-tool-button"
              disabled={busy}
              onClick={() => void loadEarlier()}
            >
              Load earlier messages
            </button>
          )}
          <section
            className="share-transcript"
            aria-label="Shared transcript"
            aria-busy={busy}
          >
            {page?.messages.map((message) => (
              <article key={message.id} data-role={message.role}>
                <strong>
                  {message.role === 'user' ? 'User' : 'Assistant'}
                </strong>
                <p>{message.text}</p>
              </article>
            ))}
            {page?.messages.length === 0 && (
              <p>No user or assistant text in this page.</p>
            )}
          </section>
        </>
      )}
      {busy && <p role="status">Loading conversation…</p>}
      {error && <p role="alert">{error}</p>}
    </main>
  );
}

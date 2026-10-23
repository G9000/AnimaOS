import { useResetOnChange } from '../../hooks/useResetOnChange';
import { useEffect, useState } from 'react';

import {
  daemon,
  type ForeignImport,
  type ForeignSession,
  type ForeignSessionPreview,
  type ForeignSource,
} from '../../lib/daemon-api';
import { ShieldIcon, XIcon } from '../icons';
import './import-conversation.css';

const SOURCES: { id: ForeignSource; label: string }[] = [
  { id: 'claude-code', label: 'Claude Code' },
  { id: 'codex', label: 'Codex' },
];

/** "~/code/uploader" when the folder is under a home directory. */
function shortCwd(cwd: string | null): string {
  if (!cwd) return '';
  const home = cwd.match(
    /^(\/Users\/[^/]+|\/home\/[^/]+|[A-Z]:\\Users\\[^\\]+)/,
  );
  return home ? `~${cwd.slice(home[0].length)}` : cwd;
}

function formatSize(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

function formatDay(ms: number): string {
  const date = new Date(ms);
  if (Number.isNaN(date.getTime())) return '';
  return date.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

/** Brings a Claude Code or Codex conversation into the agent as a chat it
 *  can search and continue. Paths never reach the page, only handles. */
export function ImportConversationDialog({
  agentId,
  agentName,
  onClose,
  onImported,
}: {
  agentId: string;
  agentName: string;
  onClose: () => void;
  onImported: (result: ForeignImport) => void;
}) {
  const [source, setSource] = useState<ForeignSource>('claude-code');
  const [sessions, setSessions] = useState<ForeignSession[] | null>(null);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [preview, setPreview] = useState<ForeignSessionPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [importing, setImporting] = useState(false);

  useResetOnChange([source], () => {
    setSessions(null);
    setSelected(null);
    setPreview(null);
    setError(null);
  });
  useEffect(() => {
    let live = true;
    daemon
      .foreignSessions(source)
      .then((page) => {
        if (!live) return;
        setSessions(page.sessions);
        setNextCursor(page.nextCursor);
        setSelected(page.sessions[0]?.handle ?? null);
      })
      .catch(
        () =>
          live &&
          setError('Could not look for conversations on this computer.'),
      );
    return () => {
      live = false;
    };
  }, [source]);

  useResetOnChange([selected], () => {
    if (selected) setPreview(null);
  });
  useEffect(() => {
    if (!selected) return;
    let live = true;
    daemon
      .foreignSessionPreview(selected)
      .then((value) => live && setPreview(value))
      .catch(() => live && setPreview(null));
    return () => {
      live = false;
    };
  }, [selected]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [onClose]);

  const loadMore = () => {
    if (!nextCursor) return;
    daemon
      .foreignSessions(source, nextCursor)
      .then((page) => {
        setSessions((known) => [...(known ?? []), ...page.sessions]);
        setNextCursor(page.nextCursor);
      })
      .catch(() => setError('Could not load more conversations.'));
  };

  const doImport = () => {
    if (!selected || importing) return;
    setImporting(true);
    setError(null);
    daemon
      .importForeignSession(agentId, selected)
      .then(onImported)
      .catch(() => setError('The conversation could not be imported.'))
      .finally(() => setImporting(false));
  };

  const label = SOURCES.find((item) => item.id === source)?.label ?? '';

  return (
    <div className="import-dialog-layer" role="presentation">
      <div className="import-dialog-dim" aria-hidden onClick={onClose} />
      <div
        className="import-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="import-dialog-title"
      >
        <div className="import-dialog-head">
          <div className="import-dialog-titles">
            <strong id="import-dialog-title">Import a conversation</strong>
            <span>
              Bring a Claude Code or Codex session into {agentName} as a chat
              you can search and continue.
            </span>
          </div>
          <button
            type="button"
            className="studio-tool-button"
            aria-label="Close"
            onClick={onClose}
          >
            <XIcon size={14} />
          </button>
        </div>
        <div className="import-dialog-tabs" role="tablist">
          {SOURCES.map((item) => (
            <button
              key={item.id}
              type="button"
              role="tab"
              aria-selected={item.id === source}
              className="import-dialog-tab"
              onClick={() => setSource(item.id)}
            >
              {item.label}
            </button>
          ))}
        </div>
        <div className="import-dialog-body">
          <div
            className="import-dialog-list"
            role="listbox"
            aria-label={`${label} conversations`}
          >
            {sessions === null && !error && (
              <p className="import-dialog-empty">Looking for conversations…</p>
            )}
            {sessions !== null && sessions.length === 0 && (
              <p className="import-dialog-empty">
                No {label} conversations on this computer.
              </p>
            )}
            {sessions?.map((session) => (
              <button
                key={session.handle}
                type="button"
                role="option"
                aria-selected={session.handle === selected}
                className="import-dialog-item"
                onClick={() => setSelected(session.handle)}
              >
                <span className="import-dialog-item-title">
                  {session.title}
                </span>
                <span className="import-dialog-item-meta">
                  <span className="import-dialog-cwd">
                    {shortCwd(session.cwd)}
                  </span>
                  <span>
                    {formatDay(session.modifiedAt)} ·{' '}
                    {formatSize(session.sizeBytes)}
                  </span>
                </span>
              </button>
            ))}
            {nextCursor && (
              <button
                type="button"
                className="import-dialog-more"
                onClick={loadMore}
              >
                Show older
              </button>
            )}
          </div>
          <div className="import-dialog-preview" aria-live="polite">
            {preview ? (
              <>
                <div className="import-dialog-preview-head">
                  <strong>{preview.title}</strong>
                  <span>{preview.messageCount} messages</span>
                </div>
                {preview.messages.map((message, index) => (
                  <div
                    key={index}
                    className="import-dialog-message"
                    data-role={message.role}
                  >
                    <span className="import-dialog-who">
                      {message.role === 'user' ? 'you' : label}
                    </span>
                    <p>{message.text}</p>
                  </div>
                ))}
              </>
            ) : (
              selected && <p className="import-dialog-empty">Reading…</p>
            )}
          </div>
        </div>
        <div className="import-dialog-foot">
          <ShieldIcon size={13} aria-hidden />
          <span className="import-dialog-note">
            {error ??
              'Keys and passwords are masked. Up to the last 2,000 messages. Importing again opens the same chat.'}
          </span>
          <button
            type="button"
            className="studio-tool-button"
            onClick={onClose}
          >
            Cancel
          </button>
          <button
            type="button"
            className="import-dialog-primary"
            disabled={!selected || importing}
            onClick={doImport}
          >
            {importing ? 'Importing…' : `Import to ${agentName}`}
          </button>
        </div>
      </div>
    </div>
  );
}

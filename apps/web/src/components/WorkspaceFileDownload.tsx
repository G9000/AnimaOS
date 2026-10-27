import { useContext, useEffect, useRef, useState, type ReactNode } from 'react';
import { useResetOnChange } from '../hooks/useResetOnChange';
import { daemon } from '../lib/daemon-api';
import { ChatExportScope } from './sessions/ChatExportDownload';

/** Only the known relative endpoint can use the authenticated owner transport. */
export function workspaceFileTarget(url: string | undefined): string | null {
  const match =
    url && /^\/api\/workspace\/file\/download\?path=([^&#]+)$/.exec(url);
  if (!match) return null;
  try {
    const path = decodeURIComponent(match[1].replace(/\+/g, ' '));
    if (
      new TextEncoder().encode(path).length > 1024 ||
      !/\.(?:txt|md|csv|docx|xlsx|pptx|pdf)$/i.test(path) ||
      path.includes('\\') ||
      path.includes(':') ||
      [...path].some(
        (character) =>
          character.charCodeAt(0) < 32 || character.charCodeAt(0) === 127,
      ) ||
      path.split('/').some((part) => !part || part === '.' || part === '..')
    )
      return null;
    return path;
  } catch {
    return null;
  }
}

export function WorkspaceFileDownload({
  url,
  children,
}: {
  url: string;
  children?: ReactNode;
}) {
  const scope = useContext(ChatExportScope);
  const path = workspaceFileTarget(url);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const controller = useRef<AbortController | null>(null);
  const urls = useRef(new Map<string, number>());
  const contextKey = JSON.stringify([url, scope?.agentId, scope?.sessionId]);
  const boundContext = useRef(contextKey);
  useResetOnChange([contextKey], () => {
    setBusy(false);
    setError(null);
  });
  useEffect(() => {
    boundContext.current = contextKey;
    const ownedUrls = urls.current;
    return () => {
      controller.current?.abort();
      controller.current = null;
      for (const [blobUrl, timer] of ownedUrls) {
        window.clearTimeout(timer);
        URL.revokeObjectURL(blobUrl);
      }
      ownedUrls.clear();
    };
  }, [contextKey]);

  if (!scope || !path)
    return <span>{children ?? 'File download unavailable'}</span>;
  const download = async () => {
    if (controller.current) return;
    const operation = new AbortController();
    controller.current = operation;
    setBusy(true);
    setError(null);
    const active = () =>
      boundContext.current === contextKey &&
      controller.current === operation &&
      !operation.signal.aborted;
    try {
      const blob = await daemon.downloadWorkspaceFile(path, operation.signal);
      if (!active()) return;
      const blobUrl = URL.createObjectURL(blob);
      const timer = window.setTimeout(() => {
        URL.revokeObjectURL(blobUrl);
        urls.current.delete(blobUrl);
      }, 0);
      urls.current.set(blobUrl, timer);
      const link = document.createElement('a');
      link.href = blobUrl;
      link.download = path.split('/').at(-1)!;
      link.click();
    } catch (caught) {
      if (active())
        setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      if (active()) {
        controller.current = null;
        setBusy(false);
      }
    }
  };
  return (
    <span>
      <button
        type="button"
        className="text-accent underline underline-offset-2"
        disabled={busy}
        onClick={() => void download()}
      >
        {busy ? 'Preparing download…' : (children ?? 'Download file')}
      </button>
      {error && (
        <span role="alert" className="ml-2 text-danger">
          {error}
        </span>
      )}
    </span>
  );
}

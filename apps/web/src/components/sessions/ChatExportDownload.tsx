import { createContext, useContext, useState, type ReactNode } from 'react';
import { daemon } from '../../lib/daemon-api';

export const ChatExportScope = createContext<{
  agentId: string;
  sessionId: string;
} | null>(null);

/** Only this known relative endpoint can become an authenticated download. */
export function chatExportTarget(
  url: string | undefined,
): { agentId: string; sessionId: string } | null {
  const match =
    url && /^\/api\/agents\/([^/]+)\/sessions\/([^/]+)\/export$/.exec(url);
  if (!match) return null;
  try {
    const agentId = decodeURIComponent(match[1]);
    const sessionId = decodeURIComponent(match[2]);
    return agentId && sessionId ? { agentId, sessionId } : null;
  } catch {
    return null;
  }
}

export function ChatExportDownload({
  url,
  filename,
  children,
}: {
  url: string;
  filename?: string;
  children?: ReactNode;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const scope = useContext(ChatExportScope);
  const target = chatExportTarget(url);
  if (
    !target ||
    !scope ||
    scope.agentId !== target.agentId ||
    scope.sessionId !== target.sessionId
  )
    return (
      <span>{children ?? 'Chat export unavailable in this conversation'}</span>
    );
  const download = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const markdown = await daemon.exportSession(
        target.agentId,
        target.sessionId,
      );
      const blobUrl = URL.createObjectURL(
        new Blob([markdown], { type: 'text/markdown;charset=utf-8' }),
      );
      try {
        const link = document.createElement('a');
        link.href = blobUrl;
        link.download =
          // eslint-disable-next-line no-control-regex -- Remove control bytes from untrusted download filenames.
          filename?.replace(/[\\/\x00-\x1f]/g, '_') || 'anima-chat.md';
        link.click();
      } finally {
        URL.revokeObjectURL(blobUrl);
      }
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      setBusy(false);
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
        {busy ? 'Preparing download…' : (children ?? 'Download chat Markdown')}
      </button>
      {error && (
        <span role="alert" className="ml-2 text-danger">
          {error}
        </span>
      )}
    </span>
  );
}

export function chatExportResult(name: string, result: string | null) {
  if (name !== 'export_chat' || !result) return null;
  try {
    const parsed: unknown = JSON.parse(result);
    if (
      !parsed ||
      typeof parsed !== 'object' ||
      !('url' in parsed) ||
      typeof parsed.url !== 'string' ||
      !chatExportTarget(parsed.url)
    )
      return null;
    return {
      url: parsed.url,
      filename:
        'filename' in parsed && typeof parsed.filename === 'string'
          ? parsed.filename
          : undefined,
    };
  } catch {
    return null;
  }
}

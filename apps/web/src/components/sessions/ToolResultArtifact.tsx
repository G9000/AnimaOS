import type { ToolResultPage } from '@animaos/sdk';
import { useEffect, useRef, useState } from 'react';
import { daemon } from '../../lib/daemon-api';

export interface ToolResultReader {
  read(
    agent: string,
    id: string,
    options?: { offset?: number; limit?: number; signal?: AbortSignal },
  ): Promise<ToolResultPage>;
  download(
    agent: string,
    id: string,
    options?: { signal?: AbortSignal },
  ): Promise<Blob>;
}
/** A reserved first-line projection survives the transcript's 2 KiB preview. */
export function savedToolResultId(result: string | null): string | null {
  return (
    result?.match(
      /^\[saved-tool-result id=(tr_[a-f0-9]{64}) bytes=\d+ capture=(?:complete|incomplete)\]\n/,
    )?.[1] ?? null
  );
}

export function ToolResultArtifact({
  agentId,
  artifactId,
  api = daemon.toolResults,
}: {
  agentId: string;
  artifactId: string;
  api?: ToolResultReader;
}) {
  const [open, setOpen] = useState(false);
  const [page, setPage] = useState<ToolResultPage | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const request = useRef<AbortController | null>(null);
  useEffect(() => () => request.current?.abort(), [agentId, artifactId]);

  async function load(offset: number) {
    request.current?.abort();
    const current = new AbortController();
    request.current = current;
    setBusy(true);
    setError(null);
    try {
      const next = await api.read(agentId, artifactId, {
        offset,
        limit: 8192,
        signal: current.signal,
      });
      if (!current.signal.aborted) setPage(next);
    } catch (reason) {
      if (!current.signal.aborted)
        setError(
          reason instanceof Error ? reason.message : 'Saved result unavailable',
        );
    } finally {
      if (!current.signal.aborted) setBusy(false);
    }
  }

  async function download() {
    setBusy(true);
    setError(null);
    try {
      const blob = await api.download(agentId, artifactId);
      const url = URL.createObjectURL(blob);
      try {
        const link = document.createElement('a');
        link.href = url;
        link.download = `${artifactId}.json`;
        link.click();
      } finally {
        URL.revokeObjectURL(url);
      }
    } catch (reason) {
      setError(
        reason instanceof Error
          ? reason.message
          : 'Saved result download failed',
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="tool-result-artifact mt-3 space-y-2 text-sm">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => {
          setOpen(!open);
          if (!open && !page) void load(0);
        }}
      >
        {open ? 'Hide saved result' : 'View saved result'}
      </button>
      {open && (
        <div aria-label="Saved tool result">
          {busy && <p role="status">Loading saved result…</p>}
          {error && <p role="alert">{error}</p>}
          {page && (
            <>
              <p>
                {page.artifact.bytes.toLocaleString()} saved bytes ·{' '}
                {page.artifact.toolName}
              </p>
              {!page.artifact.captureComplete && (
                <p className="tool-step-note">
                  The tool reported incomplete output. This artifact contains
                  only the output the daemon received.
                </p>
              )}
              <pre className="max-h-80 overflow-auto whitespace-pre-wrap break-words">{page.text}</pre>
              <p>
                Byte {page.offset} of {page.artifact.bytes}
              </p>
              <div className="flex flex-wrap gap-3">
                <button
                  type="button"
                  disabled={busy || page.offset === 0}
                  onClick={() => void load(0)}
                >
                  First page
                </button>
                <button
                  type="button"
                  disabled={busy || page.nextOffset === null}
                  onClick={() =>
                    page.nextOffset !== null && void load(page.nextOffset)
                  }
                >
                  Next page
                </button>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void download()}
                >
                  Download saved result
                </button>
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}

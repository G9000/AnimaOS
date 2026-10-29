import { useEffect, useState } from 'react';
import type { OfficeInventoryPage, PreviewEditContext, WorkspaceFileResponse } from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';
import { OfficeContentPreview } from '../documents/OfficeContentPreview';
import { useResetOnChange } from '../../hooks/useResetOnChange';

function asFile(path: string, page: OfficeInventoryPage): WorkspaceFileResponse {
  return { path, content: '', previewKind: 'office_content', sha256: page.source_sha256, officeContent: page, truncated: !page.coverage.complete };
}

/** Owner-scoped inventory, independent of the executable artifact iframe. */
export function ArtifactContentPreview({ path, agentId, sessionId, source, sha256, onSelect }: {
  path: string; agentId: string; sessionId: string; source?: string; sha256?: string;
  onSelect?: (context: PreviewEditContext | null) => void;
}) {
  const [file, setFile] = useState<WorkspaceFileResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  useResetOnChange([path, agentId, sessionId, sha256], () => { setFile(null); setError(null); });
  useEffect(() => {
    const controller = new AbortController();
    void daemon.documentEdits.inspectWorkspace(path, agentId, sessionId, { signal: controller.signal }).then((page) => {
      if (controller.signal.aborted) return;
      if (sha256 !== undefined && page.source_sha256 !== sha256) { setError('The file changed. Reload the preview before selecting content.'); return; }
      setFile(asFile(path, page));
    }).catch((caught: unknown) => {
      if (!controller.signal.aborted) setError(caught instanceof Error ? caught.message : 'Content selection is unavailable for this file.');
    });
    return () => controller.abort();
  }, [path, agentId, sessionId, sha256]);
  if (error) return <p role="alert" className="p-4 text-sm text-danger">{error}</p>;
  if (!file) return <p role="status" className="p-4 text-sm text-ink-3">Loading selectable content…</p>;
  return <OfficeContentPreview path={path} file={file} htmlSource={source} onSelect={onSelect} loadPage={async (cursor, signal) => asFile(path, await daemon.documentEdits.inspectWorkspace(path, agentId, sessionId, { cursor, signal }))} />;
}

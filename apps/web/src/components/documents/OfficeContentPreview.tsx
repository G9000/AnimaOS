import { useEffect, useRef, useState } from 'react';
import type { OfficeInventoryPage, WorkspaceFileResponse } from '@animaos/sdk';
import { revealHiddenCharacters } from '../../lib/approvals';
import { daemon } from '../../lib/daemon-api';
import { officeTargetText, validOfficeTargetValue } from './DocumentEditCard';

const object = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === 'object' && !Array.isArray(value);
const count = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) >= 0;
function validPage(value: unknown, path: string, fingerprint: string, prior: number): value is OfficeInventoryPage {
  if (!object(value) || value.version !== 1 || !['docx', 'xlsx', 'pptx'].includes(String(value.format)) || value.source_sha256 !== fingerprint || !object(value.source) || value.source.kind !== 'workspace' || value.source.display_name !== path || !Array.isArray(value.targets) || !value.targets.every((target: unknown) => object(target) && typeof target.id === 'string' && typeof target.label === 'string' && typeof target.editable === 'boolean' && validOfficeTargetValue(target.value)) || !object(value.coverage) || !count(value.coverage.total_targets) || !count(value.coverage.returned_targets) || value.coverage.returned_targets !== value.targets.length || typeof value.coverage.complete !== 'boolean' || !count(value.retained_hyperlink_count) || !Array.isArray(value.limitations) || !value.limitations.every((item: unknown) => typeof item === 'string')) return false;
  const end = prior + value.targets.length;
  return end <= value.coverage.total_targets && value.coverage.complete === (end === value.coverage.total_targets) && (value.coverage.complete ? value.next_cursor === null : typeof value.next_cursor === 'string' && value.next_cursor.length > 0 && value.targets.length > 0);
}

/** A paged content projection from the current server-owned publication receipt. */
export function OfficeContentPreview({ path, file }: { path: string; file: WorkspaceFileResponse }) {
  const fingerprint = file.sha256 ?? '';
  const valid = file.path === path && file.previewKind === 'office_content' && /^[a-f0-9]{64}$/.test(fingerprint) && validPage(file.officeContent, path, fingerprint, 0) && file.truncated === !file.officeContent.coverage.complete;
  const [pages, setPages] = useState<OfficeInventoryPage[]>(() => valid ? [file.officeContent!] : []);
  const [error, setError] = useState<string | null>(() => valid ? null : 'This document preview is incomplete or changed.');
  const [blocked, setBlocked] = useState(!valid);
  const [busy, setBusy] = useState(false);
  const controller = useRef<AbortController | null>(null);
  useEffect(() => () => controller.current?.abort(), []);
  const first = pages[0];
  const last = pages.at(-1);
  const shown = pages.reduce((total, page) => total + page.targets.length, 0);
  const next = last?.next_cursor;
  const seen = pages.map((page) => page.next_cursor).filter((cursor): cursor is string => cursor !== null);
  const load = async () => {
    if (!next || busy || blocked || controller.current) return;
    const active = new AbortController(); controller.current = active; setBusy(true); setError(null);
    try {
      const result = await daemon.readWorkspaceFile(path, { cursor: next, signal: active.signal });
      if (active.signal.aborted) return;
      const page = result.officeContent;
      if (result.path !== path || result.previewKind !== 'office_content' || result.sha256 !== fingerprint || !validPage(page, path, fingerprint, shown) || result.truncated !== !page.coverage.complete || !first || page.format !== first.format || page.coverage.total_targets !== first.coverage.total_targets || page.retained_hyperlink_count !== first.retained_hyperlink_count || (page.next_cursor !== null && seen.includes(page.next_cursor))) {
        setError('The document changed or its preview is incomplete. Refresh the file to try again.'); setBlocked(true); return;
      }
      setPages((current) => [...current, page]);
    } catch { if (!active.signal.aborted) setError('The next page could not be loaded. Try again.'); }
    finally { if (controller.current === active) { controller.current = null; if (!active.signal.aborted) setBusy(false); } }
  };
  return <div className="max-h-[65vh] space-y-4 overflow-auto p-4">
    <p>Content preview only. Open the downloaded document to review its layout.</p>
    {first && <>
      <p>{shown} of {first.coverage.total_targets} content items shown. {first.retained_hyperlink_count} retained links.</p>
      {first.limitations.map((item, index) => <p key={index}>{revealHiddenCharacters(item)}</p>)}
      <ul>{pages.flatMap((page) => page.targets).map((target, index) => <li key={`${target.id}-${index}`}><strong>{revealHiddenCharacters(target.label)}</strong>: {revealHiddenCharacters(officeTargetText(target.value))}{!target.editable && ' (preserved only)'}</li>)}</ul>
    </>}
    {error && <p role="alert">{error}</p>}
    {next && !blocked && <button type="button" className="studio-tool-button" disabled={busy} onClick={() => void load()}>{busy ? 'Loading…' : 'Load more content'}</button>}
  </div>;
}

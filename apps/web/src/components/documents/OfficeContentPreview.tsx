import { useEffect, useRef, useState } from 'react';
import { validPreviewEditContext, type OfficeInventoryPage, type OfficeTarget, type PreviewEditContext, type WorkspaceFileResponse } from '@animaos/sdk';
import { revealHiddenCharacters } from '../../lib/approvals';
import { daemon } from '../../lib/daemon-api';
import { officeTargetText, validOfficeTargetValue } from './DocumentEditCard';
import { htmlTextContext } from '../artifacts/preview-selection';

const object = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === 'object' && !Array.isArray(value);
const count = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) >= 0;
function validPage(value: unknown, path: string, fingerprint: string, prior: number): value is OfficeInventoryPage {
  if (!object(value) || value.version !== 1 || !['docx', 'xlsx', 'pptx', 'html'].includes(String(value.format)) || (value.format === 'html' ? !/\.html?$/i.test(path) : !path.toLowerCase().endsWith(`.${value.format}`)) || value.source_sha256 !== fingerprint || !object(value.source) || value.source.kind !== 'workspace' || value.source.display_name !== path || !Array.isArray(value.targets) || !value.targets.every((target: unknown) => object(target) && typeof target.id === 'string' && typeof target.label === 'string' && typeof target.editable === 'boolean' && validOfficeTargetValue(target.value)) || !object(value.coverage) || !count(value.coverage.total_targets) || !count(value.coverage.returned_targets) || value.coverage.returned_targets !== value.targets.length || typeof value.coverage.complete !== 'boolean' || !count(value.retained_hyperlink_count) || !Array.isArray(value.limitations) || !value.limitations.every((item: unknown) => typeof item === 'string')) return false;
  const end = prior + value.targets.length;
  return end <= value.coverage.total_targets && value.coverage.complete === (end === value.coverage.total_targets) && (value.coverage.complete ? value.next_cursor === null : typeof value.next_cursor === 'string' && value.next_cursor.length > 0 && value.targets.length > 0);
}

/** A paged content projection from the current server-owned publication receipt. */
export function OfficeContentPreview(props: { path: string; file: WorkspaceFileResponse; htmlSource?: string; onSelect?: (context: PreviewEditContext | null) => void; loadPage?: (cursor: string, signal: AbortSignal) => Promise<WorkspaceFileResponse> }) {
  return <OfficeContentPreviewInner key={`${props.path}:${props.file.sha256 ?? ''}:${props.file.officeContent?.source_scope ?? ''}:${props.htmlSource ?? ''}`} {...props} />;
}

function OfficeContentPreviewInner({ path, file, htmlSource, onSelect, loadPage }: Parameters<typeof OfficeContentPreview>[0]) {
  const fingerprint = file.sha256 ?? '';
  const valid = file.path === path && file.previewKind === 'office_content' && /^[a-f0-9]{64}$/.test(fingerprint) && validPage(file.officeContent, path, fingerprint, 0) && (!onSelect || /^[a-f0-9]{64}$/.test(file.officeContent.source_scope ?? '')) && file.truncated === !file.officeContent.coverage.complete;
  const [pages, setPages] = useState<OfficeInventoryPage[]>(() => valid ? [file.officeContent!] : []);
  const [error, setError] = useState<string | null>(() => valid ? null : 'This document preview is incomplete or changed.');
  const [blocked, setBlocked] = useState(!valid);
  const [busy, setBusy] = useState(false);
  const [selection, setSelection] = useState<PreviewEditContext | null>(null);
  const selectRef = useRef(onSelect);
  useEffect(() => { selectRef.current = onSelect; }, [onSelect]);
  const controller = useRef<AbortController | null>(null);
  useEffect(() => () => { controller.current?.abort(); selectRef.current?.(null); }, []);
  const first = pages[0];
  const last = pages.at(-1);
  const shown = pages.reduce((total, page) => total + page.targets.length, 0);
  const next = last?.next_cursor;
  const seen = pages.map((page) => page.next_cursor).filter((cursor): cursor is string => cursor !== null);
  const load = async () => {
    if (!next || busy || blocked || controller.current) return;
    const active = new AbortController(); controller.current = active; setBusy(true); setError(null);
    try {
      const result = loadPage ? await loadPage(next, active.signal) : await daemon.readWorkspaceFile(path, { cursor: next, signal: active.signal });
      if (active.signal.aborted) return;
      const page = result.officeContent;
      if (result.path !== path || result.previewKind !== 'office_content' || result.sha256 !== fingerprint || !validPage(page, path, fingerprint, shown) || result.truncated !== !page.coverage.complete || !first || (onSelect && page.source_scope !== first.source_scope) || page.format !== first.format || page.coverage.total_targets !== first.coverage.total_targets || page.retained_hyperlink_count !== first.retained_hyperlink_count || page.targets.some((target) => pages.some((prior) => prior.targets.some((existing) => existing.id === target.id))) || (page.next_cursor !== null && seen.includes(page.next_cursor))) {
        setError('The document changed or its preview is incomplete. Refresh the file to try again.'); setBlocked(true); setSelection(null); onSelect?.(null); return;
      }
      setPages((current) => [...current, page]);
    } catch { if (!active.signal.aborted) setError('The next page could not be loaded. Try again.'); }
    finally { if (controller.current === active) { controller.current = null; if (!active.signal.aborted) setBusy(false); } }
  };
  const choose = (target: OfficeTarget, start?: number, end?: number) => {
    if (!onSelect || blocked || !target.editable || !first) return;
    const candidate: PreviewEditContext | null = target.value.kind === 'html_text'
      ? htmlSource === undefined ? null : htmlTextContext(path, fingerprint, first.source_scope ?? '', htmlSource, target, start ?? 0, end ?? target.value.text.length)
      : { version: 1, source: { kind: 'workspace', path, sha256: fingerprint, scope: first.source_scope ?? '' }, selection: { kind: 'office_target', format: first.format, target_id: target.id, label: target.label, value: target.value } };
    setSelection(candidate && validPreviewEditContext(candidate) ? candidate : null);
  };
  const highlight = (target: OfficeTarget, element: HTMLSpanElement) => {
    const selected = window.getSelection();
    if (!selected || selected.rangeCount !== 1 || selected.isCollapsed) return;
    const range = selected.getRangeAt(0);
    // Only one original text node is a precise source range. Cross-item selections are refused.
    if (range.startContainer !== element.firstChild || range.endContainer !== element.firstChild || element.firstChild?.nodeType !== Node.TEXT_NODE) { setSelection(null); return; }
    choose(target, range.startOffset, range.endOffset);
  };
  return <div className="max-h-[65vh] space-y-4 overflow-auto p-4">
    <p>Content preview only. Open the downloaded document to review its layout.</p>
    {onSelect && <p className="text-sm text-ink-2">{first?.format === 'html' ? 'Select one text item or highlight part of it. HTML entities stay as written in the source; dynamic content and charts are unavailable here.' : 'Select an editable paragraph, cell, chart or slide item to ask for a targeted change.'}</p>}
    {first && <>
      <p>{shown} of {first.coverage.total_targets} content items shown. {first.retained_hyperlink_count} retained links.</p>
      {first.limitations.map((item, index) => <p key={index}>{revealHiddenCharacters(item)}</p>)}
      <ul className="space-y-3">{pages.flatMap((page) => page.targets).map((target, index) => <li key={`${target.id}-${index}`} className="rounded-lg border border-line p-3"><strong>{revealHiddenCharacters(target.label)}</strong>: <span data-testid={`preview-target-text-${target.id}`} className="whitespace-pre-wrap break-words" style={{ unicodeBidi: 'plaintext' }} onMouseUp={onSelect && target.value.kind === 'html_text' ? (event) => highlight(target, event.currentTarget) : undefined} onKeyUp={onSelect && target.value.kind === 'html_text' ? (event) => highlight(target, event.currentTarget) : undefined}>{onSelect && target.value.kind === 'html_text' ? target.value.text : revealHiddenCharacters(officeTargetText(target.value))}</span>{!target.editable && ' (preserved only)'}{onSelect && target.editable && <button type="button" className="studio-tool-button ml-2" disabled={blocked} aria-label={`Select ${revealHiddenCharacters(target.label)}`} onClick={() => choose(target)}>Select</button>}</li>)}</ul>
    </>}
    {error && <p role="alert">{error}</p>}
    {onSelect && <div className="sticky bottom-0 space-y-2 rounded-xl border border-line bg-panel p-3"><p className="text-sm">{selection ? selection.selection.kind === 'html_text' ? `Selected: ${revealHiddenCharacters(selection.selection.expected_text)}` : `Selected: ${revealHiddenCharacters(selection.selection.label)}` : 'Choose content to edit.'}</p><button type="button" className="studio-tool-button" disabled={!selection || blocked} onClick={() => selection && onSelect(selection)}>Add selection to draft</button></div>}
    {next && !blocked && <button type="button" className="studio-tool-button" disabled={busy} onClick={() => void load()}>{busy ? 'Loading…' : 'Load more content'}</button>}
  </div>;
}

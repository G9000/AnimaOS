import { useEffect, useRef, useState } from 'react';
import type { Approval, DocumentEditReview, OfficeInventory, OfficeTargetValue } from '@animaos/sdk';
import { revealHiddenCharacters, type ApprovalDecide } from '../../lib/approvals';

type LoadReview = (proposalId: string, agentId: string, sessionId: string, options: { signal: AbortSignal }) => Promise<unknown>;
type ObjectValue = Record<string, unknown>;
const object = (v: unknown): v is ObjectValue => v !== null && typeof v === 'object' && !Array.isArray(v);
const text = (v: unknown): v is string => typeof v === 'string';
const count = (v: unknown): v is number => Number.isSafeInteger(v) && (v as number) >= 0;
const hash = (v: unknown): v is string => text(v) && /^[a-f0-9]{64}$/.test(v);
const strings = (v: unknown): v is string[] => Array.isArray(v) && v.every(text);
const range = (v: unknown) => v === null || (object(v) && text(v.sheet_id) && text(v.start) && text(v.end));
export function validOfficeTargetValue(value: unknown): boolean {
  if (!object(value)) return false;
  switch (value.kind) {
    case 'text': return text(value.text);
    case 'chart': return text(value.title) && text(value.chart_type);
    case 'series': return range(value.categories) && range(value.values);
    case 'slide': return count(value.ordinal) && typeof value.hidden === 'boolean';
    case 'cell': {
      const cell = value.value;
      if (!object(cell)) return false;
      const validCell = cell.type === 'blank' || (cell.type === 'string' && text(cell.value)) || (cell.type === 'number' && typeof cell.value === 'number' && Number.isFinite(cell.value)) || (cell.type === 'boolean' && typeof cell.value === 'boolean');
      return validCell && text(value.sheet_id) && text(value.address) && (value.formula === null || text(value.formula)) && ['not_applicable', 'literal', 'stored_unverified', 'calculated', 'needs_external_recalculation', 'calculation_error'].includes(String(value.calculation));
    }
    default: return false;
  }
}
function inventory(value: unknown): value is OfficeInventory {
  return object(value) && value.version === 1 && ['docx', 'xlsx', 'pptx'].includes(String(value.format)) && Array.isArray(value.targets) && value.targets.every((target: unknown) => object(target) && text(target.id) && text(target.label) && typeof target.editable === 'boolean' && validOfficeTargetValue(target.value)) && Array.isArray(value.retained_hyperlinks) && value.retained_hyperlinks.every((link: unknown) => object(link) && text(link.relationship_id) && text(link.target)) && strings(value.limitations);
}
function destination(value: unknown): boolean {
  return object(value) && ((value.state === 'absent' && Object.keys(value).length === 1) || (value.state === 'present' && hash(value.sha256) && Object.keys(value).length === 2));
}
function sameDestination(left: unknown, right: unknown): boolean {
  if (!destination(left) || !destination(right) || !object(left) || !object(right)) return false;
  return left.state === right.state && (left.state === 'absent' || left.sha256 === right.sha256);
}
function witness(approval: Approval): ObjectValue | null {
  if (approval.tool !== 'apply_document_edit' || approval.argumentsTruncated) return null;
  try {
    const value: unknown = JSON.parse(approval.arguments);
    return object(value) && Object.keys(value).length === 7 && value._anima_exact_document_edit === true && text(value.proposal_id) && value.proposal_id.length > 0 && hash(value.review_sha256) && hash(value.source_sha256) && hash(value.candidate_sha256) && text(value.file_path) && destination(value.expected_destination) ? value : null;
  } catch { return null; }
}
function validReview(value: unknown, expected: ObjectValue): value is DocumentEditReview {
  if (!object(value) || value.version !== 1 || value.preview_quality !== 'structural_content' || value.proposal_id !== expected.proposal_id || value.review_sha256 !== expected.review_sha256 || value.source_sha256 !== expected.source_sha256 || value.file_path !== expected.file_path || !sameDestination(value.expected_destination, expected.expected_destination) || !object(value.source) || !text(value.source.kind) || !text(value.source.display_name) || !count(value.expires_at_ms) || value.expires_at_ms <= Date.now() || !inventory(value.before) || !inventory(value.after) || value.before.format !== value.after.format || !Array.isArray(value.changes) || value.changes.length === 0 || !value.changes.every((change: unknown) => object(change) && (change.operation_index === null || count(change.operation_index)) && text(change.label) && text(change.before) && text(change.after) && typeof change.derived === 'boolean') || !object(value.preservation) || !count(value.preservation.unchanged_parts) || !count(value.preservation.edited_parts) || value.preservation.unchanged_bytes_verified !== true || typeof value.preservation.container_normalized !== 'boolean') return false;
  try { return JSON.stringify(value).length <= 128 * 1024; } catch { return false; }
}
const safe = (value: string) => revealHiddenCharacters(value);
export function officeTargetText(value: OfficeTargetValue): string {
  switch (value.kind) {
    case 'text': return value.text;
    case 'chart': return `${value.chart_type}: ${value.title}`;
    case 'slide': return `Slide ${value.ordinal}${value.hidden ? ' (hidden)' : ''}`;
    case 'series': return `Categories: ${formatRange(value.categories)}; values: ${formatRange(value.values)}`;
    case 'cell': return `${value.sheet_id} ${value.address}: ${value.value.type === 'blank' ? '(blank)' : String(value.value.value)}${value.formula === null ? '' : `; formula: ${value.formula}`}; ${value.calculation.replaceAll('_', ' ')}`;
  }
}
function formatRange(value: { sheet_id: string; start: string; end: string } | null): string {
  return value ? `${value.sheet_id} ${value.start}:${value.end}` : '(not available)';
}
function Content({ label, value }: { label: string; value: OfficeInventory }) {
  return <section aria-label={label}><h3>{label}</h3>
    <ul>{value.targets.map((target, index) => <li key={`${target.id}-${index}`}><strong>{safe(target.label)}</strong>: {safe(officeTargetText(target.value))}{!target.editable && ' (preserved only)'}</li>)}</ul>
    {value.retained_hyperlinks.map((link, index) => <p key={`${link.relationship_id}-${index}`}>Retained link: {safe(link.target)}</p>)}
    {value.limitations.map((limit, index) => <p key={index}>{safe(limit)}</p>)}
  </section>;
}

export function DocumentEditCard({ approval, load, onDecide }: { approval: Approval; load: LoadReview; onDecide?: ApprovalDecide }) {
  const identity = JSON.stringify([approval.id, approval.agentId, approval.sessionId, approval.runId, approval.revision, approval.arguments, approval.argumentsTruncated, approval.status, approval.expiresAtMs, Boolean(onDecide)]);
  return <DocumentEditCardInner key={identity} approval={approval} load={load} onDecide={onDecide} />;
}

function DocumentEditCardInner({ approval, load, onDecide }: { approval: Approval; load: LoadReview; onDecide?: ApprovalDecide }) {
  const alive = useRef(false);
  const { agentId, sessionId, status, arguments: approvalArguments, argumentsTruncated, tool } = approval;
  const canDecide = Boolean(onDecide);
  const expected = witness(approval);
  const [review, setReview] = useState<DocumentEditReview | null>(null);
  const [failure, setFailure] = useState<string | null>(() => canDecide && status === 'pending' && !expected ? 'This document review is incomplete or unavailable.' : null);
  const [busy, setBusy] = useState(false);
  const [settled, setSettled] = useState(false);
  const [uncertain, setUncertain] = useState(false);
  const [expired, setExpired] = useState(() => approval.expiresAtMs <= Date.now());
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  useEffect(() => {
    const controller = new AbortController();
    const binding = witness({ tool, arguments: approvalArguments, argumentsTruncated } as Approval);
    if (!canDecide || status !== 'pending' || !binding) return () => controller.abort();
    void load(binding.proposal_id as string, agentId, sessionId, { signal: controller.signal }).then((result) => {
      if (controller.signal.aborted) return;
      if (!validReview(result, binding)) { setFailure('This document review changed or is incomplete.'); return; }
      setReview(result);
    }).catch(() => { if (!controller.signal.aborted) setFailure('The document review could not be loaded.'); });
    return () => controller.abort();
  }, [agentId, sessionId, status, approvalArguments, argumentsTruncated, tool, load, canDecide]);
  useEffect(() => {
    const expiresAt = Math.min(approval.expiresAtMs, review?.expires_at_ms ?? approval.expiresAtMs);
    const delay = expiresAt - Date.now();
    const timer = window.setTimeout(() => setExpired(true), Math.max(0, Math.min(delay, 2_147_483_647)));
    return () => window.clearTimeout(timer);
  }, [approval.expiresAtMs, review?.expires_at_ms]);
  const decide = async (decision: 'allow_once' | 'deny') => {
    if (!onDecide || busy || settled || uncertain || approval.status !== 'pending' || (decision === 'allow_once' && (!review || !expected || expired || Date.now() >= Math.min(review.expires_at_ms, approval.expiresAtMs)))) return;
    setBusy(true);
    try {
      const error = await onDecide(approval, { decision });
      if (!alive.current) return;
      if (error) { setUncertain(true); setFailure('The decision could not be confirmed. Refresh the chat to check its status.'); }
      else setSettled(true);
    } catch {
      if (alive.current) { setUncertain(true); setFailure('The decision could not be confirmed. Refresh the chat to check its status.'); }
    } finally { if (alive.current) setBusy(false); }
  };
  return <section className="approval-card" aria-label="Document changes">
    <header><strong>Review document changes</strong></header>
    {!onDecide && <p>Document review is available to the owner.</p>}
    {onDecide && !review && !failure && <p role="status">Loading complete review…</p>}
    {failure && <p role="alert">{failure}</p>}
    {expired && <p role="alert">This review has expired.</p>}
    {review && <>
      <p>Source: {safe(review.source.display_name)} · Destination: {safe(review.file_path)} ({review.expected_destination.state === 'absent' ? 'new file' : 'replace the reviewed existing file'})</p>
      <p>Content preview only; open the finished file to check its layout.</p>
      <h3>Changes</h3><ul>{review.changes.map((change, index) => <li key={index}><strong>{safe(change.label)}</strong>{change.derived && ' (automatic change)'}<p>Before: {safe(change.before)}</p><p>After: {safe(change.after)}</p></li>)}</ul>
      <Content label="Before" value={review.before} /><Content label="After" value={review.after} />
      <p>{review.preservation.unchanged_parts} unchanged parts verified. {review.preservation.edited_parts} changed parts.{review.preservation.container_normalized && ' The file container is normalized.'}</p>
    </>}
    {settled ? <p role="status">Decision sent.</p> : onDecide && <div className="approval-card-actions">
      <button type="button" className="studio-tool-button" disabled={!review || !expected || expired || busy || uncertain || approval.status !== 'pending'} onClick={() => void decide('allow_once')}>Apply these changes</button>
      <button type="button" className="studio-tool-button" disabled={busy || uncertain || approval.status !== 'pending'} onClick={() => void decide('deny')}>Keep original</button>
    </div>}
  </section>;
}

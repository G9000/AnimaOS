import type { DaemonClient } from './client.js';

export type OfficeFormat = 'docx' | 'xlsx' | 'pptx';
export type OfficeCalculationState = 'not_applicable' | 'literal' | 'stored_unverified' | 'calculated' | 'needs_external_recalculation' | 'calculation_error';
export type OfficeCellValue = { type: 'string'; value: string } | { type: 'number'; value: number } | { type: 'boolean'; value: boolean } | { type: 'blank' };
export interface OfficeRange { sheet_id: string; start: string; end: string }
export type OfficeTargetValue =
  | { kind: 'text'; text: string }
  | { kind: 'cell'; sheet_id: string; address: string; value: OfficeCellValue; formula: string | null; calculation: OfficeCalculationState }
  | { kind: 'chart'; title: string; chart_type: string }
  | { kind: 'series'; categories: OfficeRange | null; values: OfficeRange | null }
  | { kind: 'slide'; ordinal: number; hidden: boolean };
export interface OfficeTarget { id: string; label: string; editable: boolean; value: OfficeTargetValue }
export interface OfficeInventory {
  version: 1; format: OfficeFormat; targets: OfficeTarget[];
  retained_hyperlinks: { relationship_id: string; target: string }[]; limitations: string[];
}
export interface OfficeChange { operation_index: number | null; label: string; before: string; after: string; derived: boolean }
export interface OfficePreservation { unchanged_parts: number; edited_parts: number; unchanged_bytes_verified: boolean; container_normalized: boolean }
export type DocumentEditDestination = { state: 'absent' } | { state: 'present'; sha256: string };
export interface DocumentEditSourceLabel { kind: string; display_name: string }
export interface DocumentEditReview {
  version: 1; proposal_id: string; review_sha256: string; source: DocumentEditSourceLabel;
  source_sha256: string; file_path: string; expected_destination: DocumentEditDestination;
  expires_at_ms: number; preview_quality: 'structural_content'; before: OfficeInventory;
  after: OfficeInventory; changes: OfficeChange[]; preservation: OfficePreservation;
}
export interface OfficeInventoryPage {
  version: 1; format: OfficeFormat; source_sha256: string; source: DocumentEditSourceLabel;
  targets: OfficeTarget[]; coverage: { total_targets: number; returned_targets: number; complete: boolean };
  retained_hyperlink_count: number; limitations: string[]; next_cursor: string | null;
}
export interface DocumentEditCancellation { version: 1; proposal_id: string; cancelled: true }

function reviewPath(proposalId: string, agentId: string, sessionId: string): string {
  const query = new URLSearchParams({ agent_id: agentId, session_id: sessionId });
  return `/api/document-edits/${encodeURIComponent(proposalId)}?${query}`;
}

/** Owner-only review and cancellation. Applying uses the exact approval decision. */
export class DocumentEditsClient {
  constructor(private readonly client: DaemonClient) {}
  review(proposalId: string, agentId: string, sessionId: string, options: { signal?: AbortSignal } = {}): Promise<DocumentEditReview> {
    return this.client.requestJson(reviewPath(proposalId, agentId, sessionId), { signal: options.signal });
  }
  cancel(proposalId: string, agentId: string, sessionId: string, options: { signal?: AbortSignal } = {}): Promise<DocumentEditCancellation> {
    return this.client.requestJson(reviewPath(proposalId, agentId, sessionId), { method: 'DELETE', signal: options.signal });
  }
}

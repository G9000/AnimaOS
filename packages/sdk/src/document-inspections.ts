import type { DaemonClient } from './client.js';

export type InspectionSubject =
  | { kind: 'workspace'; file_path: string; expected_sha256: string }
  | { kind: 'attachment'; attachment_id: string; expected_sha256: string }
  | { kind: 'candidate'; proposal_id: string; expected_sha256: string };
export interface FormulaIdentity { sheet_id: string; sheet_ordinal: number; address: string; formula_sha256: string }
export type FormulaValue = { kind: 'number'; value: number } | { kind: 'text'; value: string } | { kind: 'boolean'; value: boolean } | { kind: 'empty' } | { kind: 'error'; code: number };
export interface FormulaResult { identity: FormulaIdentity; engine_formula: string; value: FormulaValue }
export type CalculationEvidence = { kind: 'not_applicable' } | { kind: 'not_requested' } | { kind: 'no_formulas' } | { kind: 'recalculated'; results: FormulaResult[] } | { kind: 'inconclusive'; reason: string };
export interface RasterInk { measurement_version: number; threshold: number; non_background_pixels: number; total_pixels: number }
export interface InspectionRenderedPage { ordinal: number; png_sha256: string; byte_length: number; width: number; height: number; ink: RasterInk }
/** Native measurements under one backend. This record is neither approval nor a quality pass. */
export interface ArtifactInspectionReport {
  schema_version: 1;
  report_sha256: string;
  source: { format: 'docx' | 'xlsx' | 'pptx'; sha256: string; byte_length: number; source_version: string };
  renderer: { backend: string; version: string; executable_sha256: string; configuration_sha256: string; font_manifest_sha256: string; rasterizer: string };
  formula_inventory: { sha256: string; formulas: FormulaIdentity[] };
  rendering: {
    pdf_sha256: string; pdf_byte_length: number; exported_page_count: number; pages: InspectionRenderedPage[];
    coverage: { kind: 'complete_exported_view' } | { kind: 'partial'; missing_pages: number[]; reason: string };
    view: 'writer_print_layout' | 'calc_printed_sheets_and_print_areas' | 'impress_exported_slides';
  };
  calculation: { evidence: CalculationEvidence; source_inventory_sha256: string; evaluated_inventory_sha256: string | null; result_sha256: string | null; total_formulas: number; inspected_formulas: number; errors: { identity: FormulaIdentity; code: number }[] };
  limitations: string[];
}
export interface InspectionPageRef { page: number; png_sha256: string; width: number; height: number }
export interface InspectionEnvelope { version: 1; report_sha256: string; report: ArtifactInspectionReport; pages: InspectionPageRef[] }

function ownerQuery(agentId: string, sessionId: string): string {
  return new URLSearchParams({ agent_id: agentId, session_id: sessionId }).toString();
}
function reportPath(hash: string, agentId: string, sessionId: string, suffix = ''): string {
  if (!/^[a-f0-9]{64}$/.test(hash)) throw new Error('Invalid inspection report identity');
  return `/api/document-inspections/${hash}${suffix}?${ownerQuery(agentId, sessionId)}`;
}

/** Explicit inspection with current source authority checked on every report and page read. */
export class DocumentInspectionsClient {
  constructor(private readonly client: DaemonClient) {}
  inspect(subject: InspectionSubject, agentId: string, sessionId: string, options: { signal?: AbortSignal } = {}): Promise<InspectionEnvelope> {
    return this.client.requestJson(`/api/document-inspections?${ownerQuery(agentId, sessionId)}`, { method: 'POST', body: { subject }, signal: options.signal });
  }
  get(hash: string, agentId: string, sessionId: string, options: { signal?: AbortSignal } = {}): Promise<InspectionEnvelope> {
    return this.client.requestJson(reportPath(hash, agentId, sessionId), { signal: options.signal });
  }
  async page(hash: string, page: number, agentId: string, sessionId: string, options: { signal?: AbortSignal } = {}): Promise<Blob> {
    if (!Number.isSafeInteger(page) || page < 1 || page > 100) throw new Error('Invalid inspection page');
    return this.client.requestBoundedBlob(reportPath(hash, agentId, sessionId, `/pages/${page}`), { signal: options.signal }, { contentType: 'image/png', maxBytes: 2 * 1024 * 1024 });
  }
}

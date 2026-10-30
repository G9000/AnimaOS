import type { UploadedAttachment } from './attachments.js';
import type { DaemonClient } from './client.js';

export interface ImageVersion {
  id: string;
  attachment: UploadedAttachment;
  parentAttachmentId: string | null;
  width: 1024;
  height: 1024;
  model: 'gpt-image-2.5-sunburst';
  createdAtMs: number;
}

/** A host byte-readback observation. It proves neither visual quality nor remote outcomes. */
export type ImageObservation = {
  operationId: string;
  versionId: string;
  attachmentId: string;
  observedAtMs: number;
  limit: 'saved_bytes_only';
} & (
  | { state: 'bytes_matched'; sha256: string; byteLength: number }
  | { state: 'unavailable'; reason: 'bytes_unavailable' }
  | { state: 'not_checked'; reason: 'budget_exceeded' | 'reader_busy' }
);

export interface ImageOperation {
  id: string;
  prompt: string;
  sourceAttachmentId: string | null;
  createdAtMs: number;
  status: 'dispatching' | 'succeeded' | 'failed' | 'outcome_unknown';
  version: ImageVersion | null;
  /** Missing on older daemons; saved metadata alone is not a readback. */
  observation?: ImageObservation;
}

export interface SessionImages {
  clearGeneration: number;
  provider: 'openai';
  model: 'gpt-image-2.5-sunburst';
  operations: ImageOperation[];
}

type RecordValue = Record<string, unknown>;
function closed(value: unknown, keys: readonly string[]): value is RecordValue {
  return value !== null && typeof value === 'object' && !Array.isArray(value) &&
    Object.keys(value).length === keys.length && keys.every((key) => Object.hasOwn(value, key));
}
const count = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) >= 0;
const id = (value: unknown): value is string => typeof value === 'string' && /^[A-Za-z0-9][A-Za-z0-9_:-]{0,127}$/.test(value);
const nullableId = (value: unknown): value is string | null => value === null || id(value);
const text = (value: unknown, maxBytes: number): value is string => typeof value === 'string' && value.length > 0 && new TextEncoder().encode(value).length <= maxBytes;

function validVersion(value: unknown, source: unknown): value is ImageVersion {
  if (!closed(value, ['id', 'attachment', 'parentAttachmentId', 'width', 'height', 'model', 'createdAtMs']) ||
    !id(value.id) || value.parentAttachmentId !== source || !nullableId(value.parentAttachmentId) ||
    value.width !== 1024 || value.height !== 1024 || value.model !== 'gpt-image-2.5-sunburst' || !count(value.createdAtMs)) return false;
  const attachment = value.attachment;
  return closed(attachment, ['id', 'name', 'mime', 'size', 'kind', 'path', 'createdAtMs']) &&
    id(attachment.id) && attachment.id !== source && text(attachment.name, 1024) &&
    attachment.mime === 'image/png' && attachment.kind === 'image' &&
    count(attachment.size) && attachment.size > 0 && attachment.size <= 10 * 1024 * 1024 &&
    text(attachment.path, 4096) && count(attachment.createdAtMs);
}

function validObservation(value: unknown, operation: RecordValue, version: ImageVersion): value is ImageObservation {
  const target = ['operationId', 'versionId', 'attachmentId', 'observedAtMs', 'limit', 'state'];
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return false;
  const observation = value as RecordValue;
  if (observation.operationId !== operation.id || observation.versionId !== version.id ||
    observation.attachmentId !== version.attachment.id || observation.limit !== 'saved_bytes_only' ||
    !count(observation.observedAtMs) || observation.observedAtMs === 0) return false;
  switch (observation.state) {
    case 'bytes_matched':
      return closed(value, [...target, 'sha256', 'byteLength']) &&
        typeof value.sha256 === 'string' && /^[a-f0-9]{64}$/.test(value.sha256) && value.byteLength === version.attachment.size;
    case 'unavailable':
      return closed(value, [...target, 'reason']) && value.reason === 'bytes_unavailable';
    case 'not_checked':
      return closed(value, [...target, 'reason']) && (value.reason === 'budget_exceeded' || value.reason === 'reader_busy');
    default:
      return false;
  }
}

function validPage(value: unknown): value is SessionImages {
  if (!closed(value, ['clearGeneration', 'provider', 'model', 'operations']) || !count(value.clearGeneration) ||
    value.provider !== 'openai' || value.model !== 'gpt-image-2.5-sunburst' ||
    !Array.isArray(value.operations) || value.operations.length > 32) return false;
  const operations = new Set<string>();
  const versions = new Set<string>();
  const attachments = new Set<string>();
  return value.operations.every((operation: unknown) => {
    const keys = ['id', 'prompt', 'sourceAttachmentId', 'createdAtMs', 'status', 'version'];
    if ((!closed(operation, keys) && !closed(operation, [...keys, 'observation'])) ||
      !id(operation.id) || operations.has(operation.id) || !text(operation.prompt, 2048) || !operation.prompt.trim() || operation.prompt.includes('\0') ||
      !nullableId(operation.sourceAttachmentId) || !count(operation.createdAtMs) ||
      typeof operation.status !== 'string' || !['dispatching', 'succeeded', 'failed', 'outcome_unknown'].includes(operation.status)) return false;
    operations.add(operation.id);
    if (operation.status !== 'succeeded') return operation.version === null && !Object.hasOwn(operation, 'observation');
    if (!validVersion(operation.version, operation.sourceAttachmentId) || versions.has(operation.version.id) || attachments.has(operation.version.attachment.id)) return false;
    if (Object.hasOwn(operation, 'observation') && !validObservation(operation.observation, operation, operation.version)) return false;
    versions.add(operation.version.id);
    attachments.add(operation.version.attachment.id);
    return true;
  });
}

/** Read-only native image ledger. Generation and editing require exact chat approval. */
export class ImagesClient {
  constructor(private readonly client: DaemonClient) {}

  async list(agentId: string, sessionId: string, options: { signal?: AbortSignal } = {}): Promise<SessionImages> {
    const blob = await this.client.requestBoundedBlob(
      `/api/agents/${encodeURIComponent(agentId)}/sessions/${encodeURIComponent(sessionId)}/images`,
      { signal: options.signal },
      { maxBytes: 512 * 1024, contentType: 'application/json' },
    );
    let value: unknown;
    try { value = JSON.parse(await blob.text()); } catch { throw new Error('Invalid image response'); }
    if (!validPage(value)) throw new Error('Invalid image response');
    return value;
  }
}

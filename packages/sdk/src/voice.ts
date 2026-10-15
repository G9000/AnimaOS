import type { DaemonClient } from './client.js';

export type VoiceAvailabilityReason =
  | 'available'
  | 'disabled'
  | 'missing_credentials'
  | 'unsupported_configuration'
  | 'unavailable';
export interface VoiceCapabilities {
  version: 1;
  provider: 'openai';
  model: 'whisper-1';
  available: boolean;
  reason: VoiceAvailabilityReason;
}
export interface VoiceTranscriptionInput {
  operationId: string;
  attachmentId: string;
  provider: 'openai';
  consent: true;
}
export interface VoiceTranscription {
  version: 1;
  operationId: string;
  attachmentId: string;
  provider: 'openai';
  model: 'whisper-1';
  text: string;
}
export interface VoiceRequestOptions {
  signal?: AbortSignal;
}

const UUID_V4 =
  /^[\da-f]{8}-[\da-f]{4}-4[\da-f]{3}-[89ab][\da-f]{3}-[\da-f]{12}$/i;
const REASONS: readonly VoiceAvailabilityReason[] = [
  'available',
  'disabled',
  'missing_credentials',
  'unsupported_configuration',
  'unavailable',
];

/** Explicit ephemeral owner operations only. No retry, upload or chat side effect. */
export class VoiceClient {
  constructor(private readonly client: DaemonClient) {}

  async capabilities(
    agentId: string,
    sessionId: string,
    options: VoiceRequestOptions = {},
  ): Promise<VoiceCapabilities> {
    const value = await this.client.requestJson<unknown>(
      voicePath(agentId, sessionId),
      { signal: options.signal },
    );
    const item = closedRecord(value, [
      'version',
      'provider',
      'model',
      'available',
      'reason',
    ]);
    if (
      !item ||
      item.version !== 1 ||
      item.provider !== 'openai' ||
      item.model !== 'whisper-1' ||
      typeof item.available !== 'boolean' ||
      !REASONS.includes(item.reason as VoiceAvailabilityReason) ||
      item.available !== (item.reason === 'available')
    )
      throw new Error('Invalid voice response');
    return item as unknown as VoiceCapabilities;
  }

  async transcribe(
    agentId: string,
    sessionId: string,
    input: VoiceTranscriptionInput,
    options: VoiceRequestOptions = {},
  ): Promise<VoiceTranscription> {
    if (
      !UUID_V4.test(input.operationId) ||
      typeof input.attachmentId !== 'string' ||
      !input.attachmentId ||
      input.attachmentId.length > 128 ||
      /[\s\p{Cf}\p{Cc}]/u.test(input.attachmentId) ||
      input.provider !== 'openai' ||
      input.consent !== true
    )
      throw new Error('Invalid voice operation');
    const value = await this.client.requestJson<unknown>(
      `${voicePath(agentId, sessionId)}/transcriptions`,
      {
        method: 'POST',
        signal: options.signal,
        body: {
          operationId: input.operationId,
          attachmentId: input.attachmentId,
          provider: 'openai',
          consent: true,
        },
      },
    );
    const item = closedRecord(value, [
      'version',
      'operationId',
      'attachmentId',
      'provider',
      'model',
      'text',
    ]);
    if (
      !item ||
      item.version !== 1 ||
      item.operationId !== input.operationId ||
      item.attachmentId !== input.attachmentId ||
      item.provider !== 'openai' ||
      item.model !== 'whisper-1' ||
      typeof item.text !== 'string' ||
      !item.text.trim() ||
      item.text.includes('\0') ||
      new TextEncoder().encode(item.text).byteLength > 16 * 1024
    )
      throw new Error('Invalid voice response');
    return {
      version: 1,
      operationId: input.operationId,
      attachmentId: input.attachmentId,
      provider: 'openai',
      model: 'whisper-1',
      text: item.text.trim(),
    };
  }

  /** Acknowledges local cancellation; it cannot establish remote inference or billing stopped. */
  async cancel(
    agentId: string,
    sessionId: string,
    operationId: string,
    options: VoiceRequestOptions = {},
  ): Promise<void> {
    if (!UUID_V4.test(operationId)) throw new Error('Invalid voice operation');
    await this.client.requestJson<unknown>(
      `${voicePath(agentId, sessionId)}/transcriptions/${encodeURIComponent(operationId)}`,
      { method: 'DELETE', signal: options.signal },
    );
  }
}

function voicePath(agentId: string, sessionId: string): string {
  return `/api/agents/${encodeURIComponent(agentId)}/sessions/${encodeURIComponent(sessionId)}/voice`;
}
function closedRecord(
  value: unknown,
  keys: readonly string[],
): Record<string, unknown> | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  const actual = Object.keys(value);
  return actual.length === keys.length &&
    actual.every((key) => keys.includes(key))
    ? (value as Record<string, unknown>)
    : null;
}

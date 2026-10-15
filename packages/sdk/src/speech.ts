import type { DaemonClient } from './client.js';

export type SpeechAvailabilityReason =
  | 'available'
  | 'disabled'
  | 'missing_credentials'
  | 'unsupported_configuration'
  | 'unavailable';
export interface SpeechCapabilities {
  version: 1;
  provider: 'openai';
  model: 'gpt-4o-mini-tts';
  voice: 'alloy';
  format: 'wav';
  available: boolean;
  reason: SpeechAvailabilityReason;
  maxInputChars: 2048;
  maxInputBytes: 8192;
  maxAudioBytes: 5760044;
  maxDurationMs: 120000;
}
export interface SpeechGenerateInput {
  operationId: string;
  messageId: string;
  text: string;
  consent: true;
}
export interface SpeechRequestOptions {
  signal?: AbortSignal;
}

const UUID_V4 =
  /^[\da-f]{8}-[\da-f]{4}-4[\da-f]{3}-[89ab][\da-f]{3}-[\da-f]{12}$/i;
const SAFE_ID = /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/;
const REASONS: readonly string[] = [
  'available',
  'disabled',
  'missing_credentials',
  'unsupported_configuration',
  'unavailable',
];
const MAX_WAV_BYTES = 5760044;

export function speechTextIsValid(text: string): boolean {
  return (
    typeof text === 'string' &&
    text.trim().length > 0 &&
    !text.includes('\0') &&
    ![...text].some((character) => {
      const codePoint = character.codePointAt(0)!;
      return codePoint >= 0xd800 && codePoint <= 0xdfff;
    }) &&
    [...text].length <= 2048 &&
    new TextEncoder().encode(text).byteLength <= 8192
  );
}

/** Native output is an explicit operation separate from upload/transcription. */
export class SpeechClient {
  constructor(private readonly client: DaemonClient) {}

  async capabilities(
    agentId: string,
    sessionId: string,
    options: SpeechRequestOptions = {},
  ): Promise<SpeechCapabilities> {
    const response = await this.client.requestBoundedBlob(
      speechPath(agentId, sessionId),
      { signal: options.signal },
      { maxBytes: 4096, contentType: 'application/json' },
    );
    let value: unknown;
    try {
      value = JSON.parse(await response.text());
    } catch {
      throw new Error('Invalid speech response');
    }
    if (
      !closedRecord(value, [
        'version',
        'provider',
        'model',
        'voice',
        'format',
        'available',
        'reason',
        'maxInputChars',
        'maxInputBytes',
        'maxAudioBytes',
        'maxDurationMs',
      ]) ||
      value.version !== 1 ||
      value.provider !== 'openai' ||
      value.model !== 'gpt-4o-mini-tts' ||
      value.voice !== 'alloy' ||
      value.format !== 'wav' ||
      typeof value.available !== 'boolean' ||
      !REASONS.includes(value.reason as string) ||
      value.available !== (value.reason === 'available') ||
      value.maxInputChars !== 2048 ||
      value.maxInputBytes !== 8192 ||
      value.maxAudioBytes !== MAX_WAV_BYTES ||
      value.maxDurationMs !== 120000
    )
      throw new Error('Invalid speech response');
    return value as unknown as SpeechCapabilities;
  }

  async generate(
    agentId: string,
    sessionId: string,
    input: SpeechGenerateInput,
    options: SpeechRequestOptions = {},
  ): Promise<Blob> {
    if (
      !UUID_V4.test(input.operationId) ||
      !SAFE_ID.test(input.messageId) ||
      !speechTextIsValid(input.text) ||
      input.consent !== true ||
      Object.keys(input).length !== 4
    )
      throw new Error('Invalid speech operation');
    const blob = await this.client.requestBoundedBlob(
      speechPath(agentId, sessionId),
      {
        method: 'POST',
        signal: options.signal,
        body: JSON.stringify({
          operationId: input.operationId,
          messageId: input.messageId,
          text: input.text,
          consent: true,
        }),
        headers: { 'content-type': 'application/json' },
      },
      { maxBytes: MAX_WAV_BYTES, contentType: 'audio/wav' },
    );
    await validateCanonicalWav(blob);
    return blob;
  }

  async cancel(
    agentId: string,
    sessionId: string,
    operationId: string,
    options: SpeechRequestOptions = {},
  ): Promise<void> {
    if (!UUID_V4.test(operationId)) throw new Error('Invalid speech operation');
    await this.client.requestNoBody(
      `${speechPath(agentId, sessionId)}/${encodeURIComponent(operationId)}`,
      {
        method: 'DELETE',
        signal: options.signal,
      },
      204,
    );
  }
}

function speechPath(agentId: string, sessionId: string): string {
  return `/api/agents/${encodeURIComponent(agentId)}/sessions/${encodeURIComponent(sessionId)}/voice/speech`;
}

function closedRecord(
  value: unknown,
  keys: readonly string[],
): value is Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
  const actual = Object.keys(value);
  return (
    actual.length === keys.length && actual.every((key) => keys.includes(key))
  );
}

async function validateCanonicalWav(blob: Blob): Promise<void> {
  if (blob.size < 46 || blob.size > MAX_WAV_BYTES || blob.size % 2 !== 0)
    throw new Error('Invalid speech audio');
  const header = new Uint8Array(await blob.slice(0, 44).arrayBuffer());
  const ascii = (offset: number, text: string) =>
    [...text].every((c, i) => header[offset + i] === c.charCodeAt(0));
  const view = new DataView(header.buffer);
  const payload = blob.size - 44;
  if (
    !ascii(0, 'RIFF') ||
    view.getUint32(4, true) !== blob.size - 8 ||
    !ascii(8, 'WAVE') ||
    !ascii(12, 'fmt ') ||
    view.getUint32(16, true) !== 16 ||
    view.getUint16(20, true) !== 1 ||
    view.getUint16(22, true) !== 1 ||
    view.getUint32(24, true) !== 24000 ||
    view.getUint32(28, true) !== 48000 ||
    view.getUint16(32, true) !== 2 ||
    view.getUint16(34, true) !== 16 ||
    !ascii(36, 'data') ||
    view.getUint32(40, true) !== payload ||
    payload < 2 ||
    payload % 2 !== 0 ||
    payload > 5760000
  )
    throw new Error('Invalid speech audio');
}

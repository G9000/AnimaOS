import { describe, expect, it, vi } from 'vitest';
import { createDaemonClient, DaemonHttpError } from './client.js';

const OPERATION = '123e4567-e89b-42d3-a456-426614174000';
const capability = {
  version: 1,
  provider: 'openai',
  model: 'gpt-4o-mini-tts',
  voice: 'alloy',
  format: 'wav',
  available: true,
  reason: 'available',
  maxInputChars: 2048,
  maxInputBytes: 8192,
  maxAudioBytes: 5760044,
  maxDurationMs: 120000,
};
function wav() {
  const data = new Uint8Array(12044);
  const view = new DataView(data.buffer);
  const ascii = (offset: number, text: string) =>
    [...text].forEach((c, i) => (data[offset + i] = c.charCodeAt(0)));
  ascii(0, 'RIFF');
  view.setUint32(4, 12036, true);
  ascii(8, 'WAVE');
  ascii(12, 'fmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, 24000, true);
  view.setUint32(28, 48000, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  ascii(36, 'data');
  view.setUint32(40, 12000, true);
  return data;
}
const input = {
  operationId: OPERATION,
  messageId: 'reply-1',
  text: 'Hello',
  consent: true as const,
};

describe('SpeechClient', () => {
  it('checks the exact capability contract and fixed route', async () => {
    const fetch = vi.fn(
      async (
        _input: Parameters<typeof globalThis.fetch>[0],
        _init?: RequestInit,
      ) => Response.json(capability),
    );
    const speech = createDaemonClient({
      baseUrl: '',
      fetch: fetch as typeof globalThis.fetch,
    }).speech;
    expect(await speech.capabilities('a', 's')).toEqual(capability);
    expect(fetch.mock.calls[0]?.[0]).toBe(
      '/api/agents/a/sessions/s/voice/speech',
    );
    fetch.mockImplementationOnce(async () =>
      Response.json({ ...capability, voice: 'custom' }),
    );
    await expect(speech.capabilities('a', 's')).rejects.toThrow(
      'Invalid speech response',
    );
  });

  it('refuses invalid input locally and accepts one canonical WAV', async () => {
    const bytes = wav();
    const fetch = vi.fn(
      async (
        _input: Parameters<typeof globalThis.fetch>[0],
        _init?: RequestInit,
      ) =>
        new Response(bytes, {
          headers: {
            'content-type': 'audio/wav',
            'content-length': String(bytes.length),
          },
        }),
    );
    const speech = createDaemonClient({
      baseUrl: '',
      fetch: fetch as typeof globalThis.fetch,
    }).speech;
    await expect(
      speech.generate('a', 's', { ...input, text: 'x'.repeat(2049) }),
    ).rejects.toThrow('Invalid speech operation');
    await expect(
      speech.generate('a', 's', { ...input, text: '😀'.repeat(2049) }),
    ).rejects.toThrow('Invalid speech operation');
    await expect(
      speech.generate('a', 's', { ...input, text: '\ud800' }),
    ).rejects.toThrow('Invalid speech operation');
    await expect(
      speech.generate('a', 's', { ...input, messageId: '../bad' }),
    ).rejects.toThrow('Invalid speech operation');
    expect(fetch).not.toHaveBeenCalled();
    const blob = await speech.generate('a', 's', input);
    expect(blob.size).toBe(12044);
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(fetch.mock.calls[0]?.[0]).toBe(
      '/api/agents/a/sessions/s/voice/speech',
    );
  });

  it('rejects malformed WAV and oversized streams, and discards HTTP error bodies', async () => {
    const malformed = wav();
    malformed[22] = 2;
    let response: Response = new Response(malformed, {
      headers: { 'content-type': 'audio/wav' },
    });
    const fetch = vi.fn(
      async (
        _input: Parameters<typeof globalThis.fetch>[0],
        _init?: RequestInit,
      ) => response,
    );
    const client = createDaemonClient({
      baseUrl: '',
      fetch: fetch as typeof globalThis.fetch,
    });
    await expect(client.speech.generate('a', 's', input)).rejects.toThrow(
      'Invalid speech audio',
    );
    response = new Response(new Uint8Array(5760045), {
      headers: { 'content-type': 'audio/wav' },
    });
    await expect(client.speech.generate('a', 's', input)).rejects.toThrow(
      'Bounded response too large',
    );
    response = new Response('private provider details', { status: 503 });
    await expect(client.speech.generate('a', 's', input)).rejects.toMatchObject(
      { status: 503, body: null } satisfies Partial<DaemonHttpError>,
    );
  });

  it('caps streamed bytes and cancels the reader without parsing its body', async () => {
    const cancelled = vi.fn();
    const body = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new Uint8Array(5760045));
      },
      cancel: cancelled,
    });
    const fetch = vi.fn(
      async (
        _input: Parameters<typeof globalThis.fetch>[0],
        _init?: RequestInit,
      ) => new Response(body, { headers: { 'content-type': 'audio/wav' } }),
    );
    const client = createDaemonClient({
      baseUrl: '',
      fetch: fetch as typeof globalThis.fetch,
    });
    await expect(client.speech.generate('a', 's', input)).rejects.toThrow(
      'Bounded response too large',
    );
    expect(cancelled).toHaveBeenCalledOnce();
  });

  it('uses status-only cancellation with its own signal', async () => {
    const fetch = vi.fn(
      async (
        _input: Parameters<typeof globalThis.fetch>[0],
        _init?: RequestInit,
      ) => new Response(null, { status: 204 }),
    );
    const client = createDaemonClient({
      baseUrl: '',
      fetch: fetch as typeof globalThis.fetch,
    });
    const signal = new AbortController().signal;
    await client.speech.cancel('a', 's', OPERATION, { signal });
    expect(fetch.mock.calls[0]?.[0]).toBe(
      `/api/agents/a/sessions/s/voice/speech/${OPERATION}`,
    );
    expect(fetch.mock.calls[0]?.[1]?.signal).toBe(signal);
  });

  it('rejects a declared length that differs from the streamed WAV bytes', async () => {
    const bytes = wav();
    const fetch = vi.fn(
      async (
        _input: Parameters<typeof globalThis.fetch>[0],
        _init?: RequestInit,
      ) =>
        new Response(bytes, {
          headers: {
            'content-type': 'audio/wav',
            'content-length': String(bytes.length + 1),
          },
        }),
    );
    const speech = createDaemonClient({
      baseUrl: '',
      fetch: fetch as typeof globalThis.fetch,
    }).speech;
    await expect(speech.generate('a', 's', input)).rejects.toThrow(
      'Invalid bounded response length',
    );
  });

  it('waits for an oversized stream cancellation to settle before rejecting', async () => {
    let settleCancel!: () => void;
    const cancellation = new Promise<void>((resolve) => {
      settleCancel = resolve;
    });
    const cancelled = vi.fn(() => cancellation);
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new Uint8Array(5760045));
      },
      cancel: cancelled,
    });
    const fetch = vi.fn(
      async (
        _input: Parameters<typeof globalThis.fetch>[0],
        _init?: RequestInit,
      ) => new Response(stream, { headers: { 'content-type': 'audio/wav' } }),
    );
    const speech = createDaemonClient({
      baseUrl: '',
      fetch: fetch as typeof globalThis.fetch,
    }).speech;
    let settled = false;
    const result = speech.generate('a', 's', input).finally(() => {
      settled = true;
    });
    try {
      await vi.waitFor(() => expect(cancelled).toHaveBeenCalledOnce());
      expect(settled).toBe(false);
    } finally {
      settleCancel();
    }
    await expect(result).rejects.toThrow('Bounded response too large');
    expect(settled).toBe(true);
  });

  it.each([
    { available: true, reason: 'disabled' },
    { available: false, reason: 'available' },
    { maxAudioBytes: 5760045 },
    { unexpected: 'provider details' },
  ])('rejects mismatched or extended capability replies', async (patch) => {
    const fetch = vi.fn(
      async (
        _input: Parameters<typeof globalThis.fetch>[0],
        _init?: RequestInit,
      ) => Response.json({ ...capability, ...patch }),
    );
    const speech = createDaemonClient({
      baseUrl: '',
      fetch: fetch as typeof globalThis.fetch,
    }).speech;
    await expect(speech.capabilities('a', 's')).rejects.toThrow(
      'Invalid speech response',
    );
  });
});

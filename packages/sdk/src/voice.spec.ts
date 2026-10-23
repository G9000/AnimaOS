import { describe, expect, it, vi } from 'vitest';
import { createDaemonClient, VoiceClient } from './index.js';

const operationId = '12345678-1234-4234-9234-123456789abc';
const input = {
  operationId,
  attachmentId: 'att-a',
  provider: 'openai' as const,
  consent: true as const,
};
const capabilities = {
  version: 1,
  provider: 'openai',
  model: 'whisper-1',
  available: true,
  reason: 'available',
};
const transcript = {
  version: 1,
  operationId,
  attachmentId: 'att-a',
  provider: 'openai',
  model: 'whisper-1',
  text: 'Hello world',
};

describe('explicit native voice client', () => {
  it('uses encoded owner routes, exact consent fields and independent abort signals without retries', async () => {
    const fetch = vi.fn(async (_url: unknown, init?: RequestInit) =>
      init?.method === 'DELETE'
        ? new Response(null, { status: 204 })
        : Response.json(init?.method === 'POST' ? transcript : capabilities),
    );
    const client = createDaemonClient({ baseUrl: '', fetch });
    expect(client.voice).toBeInstanceOf(VoiceClient);
    expect(fetch).not.toHaveBeenCalled();
    const signal = new AbortController().signal;
    expect(
      await client.voice.capabilities('agent/a', 'session/b', { signal }),
    ).toEqual(capabilities);
    expect(
      await client.voice.transcribe(
        'agent/a',
        'session/b',
        { ...input, secret: 'not sent' } as typeof input,
        { signal },
      ),
    ).toEqual(transcript);
    await client.voice.cancel('agent/a', 'session/b', operationId);
    expect(fetch).toHaveBeenCalledTimes(3);
    const calls = fetch.mock.calls as unknown as Array<[string, RequestInit]>;
    expect(calls.map(([url]) => url)).toEqual([
      '/api/agents/agent%2Fa/sessions/session%2Fb/voice',
      '/api/agents/agent%2Fa/sessions/session%2Fb/voice/transcriptions',
      `/api/agents/agent%2Fa/sessions/session%2Fb/voice/transcriptions/${operationId}`,
    ]);
    expect(calls[0][1].signal).toBe(signal);
    expect(calls[1][1].signal).toBe(signal);
    expect(JSON.parse(calls[1][1].body as string)).toEqual(input);
    expect(calls[2][1].method).toBe('DELETE');
    expect(calls[2][1].signal).toBeUndefined();
  });

  it.each([
    { operationId: '12345678-1234-1234-9234-123456789abc' },
    { provider: 'other' },
    { consent: false },
    { attachmentId: '' },
  ])('rejects invalid consent operation %j before a request', async (patch) => {
    const fetch = vi.fn();
    const client = createDaemonClient({ fetch });
    await expect(
      client.voice.transcribe('a', 's', { ...input, ...patch } as typeof input),
    ).rejects.toThrow('Invalid voice operation');
    expect(fetch).not.toHaveBeenCalled();
  });

  it.each([
    { version: 2 },
    { operationId: 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa' },
    { attachmentId: 'different' },
    { provider: 'other' },
    { model: 'other' },
    { text: '' },
    { text: '   ' },
    { text: 'bad\0text' },
    { text: 'é'.repeat(8193) },
    { privateBody: 'must not escape' },
  ])(
    'rejects invalid or mismatched transcription response %j',
    async (patch) => {
      const fetch = vi.fn(async () =>
        Response.json({ ...transcript, ...patch }),
      );
      const client = createDaemonClient({ fetch });
      await expect(client.voice.transcribe('a', 's', input)).rejects.toThrow(
        'Invalid voice response',
      );
      expect(fetch).toHaveBeenCalledOnce();
    },
  );

  it.each([
    { reason: 'unknown' },
    { available: false },
    { provider: 'other' },
    { model: 'other' },
    { extra: true },
  ])('fails closed on malformed capabilities %j', async (patch) => {
    const client = createDaemonClient({
      fetch: async () => Response.json({ ...capabilities, ...patch }),
    });
    await expect(client.voice.capabilities('a', 's')).rejects.toThrow(
      'Invalid voice response',
    );
  });

  it('does not retry HTTP failures or aborts', async () => {
    const fetch = vi.fn(async () =>
      Response.json({ error: 'Unavailable' }, { status: 503 }),
    );
    const client = createDaemonClient({ fetch });
    await expect(
      client.voice.transcribe('a', 's', input),
    ).rejects.toMatchObject({ status: 503 });
    expect(fetch).toHaveBeenCalledOnce();
    const abort = new DOMException('Aborted', 'AbortError');
    fetch.mockImplementationOnce(async () => {
      throw abort;
    });
    await expect(client.voice.transcribe('a', 's', input)).rejects.toBe(abort);
    expect(fetch).toHaveBeenCalledTimes(2);
  });
});

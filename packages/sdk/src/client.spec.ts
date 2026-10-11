import { getEventListeners } from 'node:events';
import { describe, expect, it, vi } from 'vitest';

import {
  createDaemonClient,
  DaemonConnectionError,
  DaemonHttpError,
} from './client.js';

describe('daemon subscription cleanup', () => {
  it('releases caller abort listeners after failed connection attempts', async () => {
    const controller = new AbortController();
    const client = createDaemonClient({
      fetch: async () => {
        throw new TypeError('connection refused');
      },
    });

    for (let attempt = 0; attempt < 3; attempt += 1) {
      await expect(
        client.subscribe('/events', { signal: controller.signal }).next(),
      ).rejects.toBeInstanceOf(DaemonConnectionError);
      expect(getEventListeners(controller.signal, 'abort')).toHaveLength(0);
    }
  });

  it('releases caller abort listeners when the response body cannot be acquired', async () => {
    const controller = new AbortController();
    const response = new Response(new ReadableStream());
    const heldReader = response.body!.getReader();
    const client = createDaemonClient({ fetch: async () => response });

    try {
      await expect(
        client.subscribe('/events', { signal: controller.signal }).next(),
      ).rejects.toThrow();
      expect(getEventListeners(controller.signal, 'abort')).toHaveLength(0);
    } finally {
      await heldReader.cancel();
      heldReader.releaseLock();
    }
  });
});

describe('daemon response bodies', () => {
  function failingBody(error: unknown): Response {
    return new Response(
      new ReadableStream({
        start(controller) {
          controller.error(error);
        },
      }),
      { status: 200, headers: { 'content-type': 'application/json' } },
    );
  }

  it('reports a body that fails after the headers as a connection error', async () => {
    const cause = new TypeError('network connection was lost');
    const client = createDaemonClient({
      fetch: async () => failingBody(cause),
    });

    const json = client.requestJson('/api/sessions');
    await expect(json).rejects.toBeInstanceOf(DaemonConnectionError);
    await expect(json).rejects.toMatchObject({ cause });
    await expect(client.requestText('/api/export')).rejects.toBeInstanceOf(
      DaemonConnectionError,
    );
  });

  it('keeps an abort while reading the body an abort', async () => {
    const abort = new DOMException('The operation was aborted.', 'AbortError');
    const client = createDaemonClient({
      fetch: async () => failingBody(abort),
    });

    await expect(client.requestJson('/api/sessions')).rejects.toMatchObject({
      name: 'AbortError',
    });
  });
});

describe('daemon binary responses', () => {
  it('retains caller headers and request options without credentials in the URL', async () => {
    const fetch = vi.fn(
      async () =>
        new Response('plain bytes', {
          headers: { 'content-type': 'text/plain' },
        }),
    );
    const client = createDaemonClient({ baseUrl: '/daemon', fetch });
    const signal = new AbortController().signal;
    const blob = await client.requestBlob('file', {
      headers: { authorization: 'Bearer owner', accept: 'image/png' },
      credentials: 'same-origin',
      signal,
    });
    expect(await blob.text()).toBe('plain bytes');
    const [url, init] = fetch.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe('/daemon/file');
    expect(init.credentials).toBe('same-origin');
    expect(init.signal).toBe(signal);
    expect(new Headers(init.headers).get('authorization')).toBe('Bearer owner');
    expect(new Headers(init.headers).get('accept')).toBe('image/png');
  });

  it.each([
    [
      Response.json({ error: 'Access denied' }, { status: 403 }),
      { error: 'Access denied' },
    ],
    [new Response('Unavailable', { status: 503 }), 'Unavailable'],
  ] as const)(
    'reads an HTTP failure body and never consumes it as a Blob',
    async (response, body) => {
      const blob = vi.spyOn(response, 'blob');
      const client = createDaemonClient({ fetch: async () => response });
      const request = client.requestBlob('/private');
      await expect(request).rejects.toBeInstanceOf(DaemonHttpError);
      await expect(request).rejects.toMatchObject({
        status: response.status,
        body,
      });
      expect(blob).not.toHaveBeenCalled();
    },
  );

  it('wraps transport errors with the resolved daemon URL', async () => {
    const cause = new TypeError('Connection lost');
    const client = createDaemonClient({
      baseUrl: '/daemon',
      fetch: async () => {
        throw cause;
      },
    });
    const request = client.requestBlob('/binary');
    await expect(request).rejects.toBeInstanceOf(DaemonConnectionError);
    await expect(request).rejects.toMatchObject({
      cause,
      message: 'Failed to reach daemon at /daemon/binary',
    });
  });

  it.each([200, 403])(
    'wraps body read failures for status %s',
    async (status) => {
      const cause = new TypeError('Body read failed');
      const client = createDaemonClient({
        fetch: async () =>
          new Response(
            new ReadableStream({
              start(controller) {
                controller.error(cause);
              },
            }),
            { status },
          ),
      });
      const request = client.requestBlob('/binary');
      await expect(request).rejects.toBeInstanceOf(DaemonConnectionError);
      await expect(request).rejects.toMatchObject({ cause });
    },
  );

  it('preserves AbortError while reading binary bytes', async () => {
    const abort = new DOMException('Aborted', 'AbortError');
    const client = createDaemonClient({
      fetch: async () =>
        new Response(
          new ReadableStream({
            start(controller) {
              controller.error(abort);
            },
          }),
        ),
    });
    await expect(client.requestBlob('/binary')).rejects.toBe(abort);
  });
});

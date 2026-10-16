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

describe('m68_explicit_empty_bounded_downloads', () => {
  it.each([undefined, false])(
    'keeps_the_existing_nonempty_default_%s',
    async (allowEmpty) => {
      const client = createDaemonClient({
        fetch: async () =>
          new Response(new Uint8Array(0), {
            headers: { 'content-type': 'audio/wav', 'content-length': '0' },
          }),
      });
      await expect(
        client.requestBoundedBlob(
          '/voice-fixture',
          {},
          {
            maxBytes: 16,
            contentType: 'audio/wav',
            ...(allowEmpty === undefined ? {} : { allowEmpty }),
          },
        ),
      ).rejects.toThrow('Invalid bounded response length');
    },
  );

  it.each(['empty stream', 'null body'])(
    'allows_an_explicit_zero_byte_%s',
    async (kind) => {
      const client = createDaemonClient({
        fetch: async () =>
          new Response(kind === 'null body' ? null : new Uint8Array(0), {
            headers: {
              'content-type': 'application/octet-stream',
              'content-length': '0',
            },
          }),
      });
      const blob = await client.requestBoundedBlob(
        '/report-fixture',
        {},
        {
          maxBytes: 16,
          contentType: 'application/octet-stream',
          allowEmpty: true,
        },
      );
      expect(blob.size).toBe(0);
      expect(blob.type).toBe('application/octet-stream');
    },
  );

  it.each([
    { length: '1', body: null, type: 'application/octet-stream' },
    {
      length: '0',
      body: new Uint8Array([120]),
      type: 'application/octet-stream',
    },
    { length: '0', body: null, type: 'application/json' },
  ])(
    'does_not_relax_length_or_type_validation_$length_$type',
    async ({ length, body, type }) => {
      const client = createDaemonClient({
        fetch: async () =>
          new Response(body, {
            headers: { 'content-type': type, 'content-length': length },
          }),
      });
      await expect(
        client.requestBoundedBlob(
          '/report-fixture',
          {},
          {
            maxBytes: 16,
            contentType: 'application/octet-stream',
            allowEmpty: true,
          },
        ),
      ).rejects.toThrow();
    },
  );

  it('still_bounds_decoded_stream_bytes_and_cancels_rejected_bodies', async () => {
    const cancelled = vi.fn();
    const response = new Response(
      new ReadableStream<Uint8Array>({
        start(controller) {
          controller.enqueue(new Uint8Array(17));
        },
        cancel: cancelled,
      }),
      { headers: { 'content-type': 'application/octet-stream' } },
    );
    const client = createDaemonClient({ fetch: async () => response });
    await expect(
      client.requestBoundedBlob(
        '/report-fixture',
        {},
        {
          maxBytes: 16,
          contentType: 'application/octet-stream',
          allowEmpty: true,
        },
      ),
    ).rejects.toThrow('Bounded response too large');
    expect(cancelled).toHaveBeenCalledTimes(1);
    expect(response.body?.locked).toBe(false);
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

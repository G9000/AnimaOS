import { describe, expect, it, vi } from 'vitest';
import {
  AttachmentsClient,
  createDaemonClient,
  DaemonHttpError,
  type UploadedAttachment,
} from './index.js';

const attachment: UploadedAttachment = {
  id: 'att_1',
  name: 'photo.png',
  mime: 'image/png',
  size: 3,
  kind: 'image',
  path: 'attachments/photo.png',
  createdAtMs: 1,
};

describe('attachments client', () => {
  it('exposes the resource and uploads exactly one named FormData file on the encoded session route', async () => {
    const fetch = vi.fn(async () =>
      Response.json({ attachment }, { status: 201 }),
    );
    const client = createDaemonClient({ baseUrl: '', fetch });
    const signal = new AbortController().signal;
    const file = new Blob([new Uint8Array([1, 2, 3])], { type: 'image/png' });
    expect(client.attachments).toBeInstanceOf(AttachmentsClient);
    expect(
      await client.attachments.upload('agent/a', 'chat:1?', file, 'photo.png', {
        signal,
      }),
    ).toEqual({ attachment });
    const [url, init] = fetch.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe('/api/agents/agent%2Fa/sessions/chat%3A1%3F/attachments');
    expect(init.method).toBe('POST');
    expect(init.signal).toBe(signal);
    const headers = new Headers(init.headers);
    expect(headers.get('accept')).toBe('application/json');
    expect(headers.has('content-type')).toBe(false);
    expect(init.body).toBeInstanceOf(FormData);
    const parts = Array.from((init.body as FormData).entries());
    expect(parts).toHaveLength(1);
    expect(parts[0][0]).toBe('file');
    const submitted = parts[0][1] as File;
    expect(submitted.name).toBe('photo.png');
    expect(submitted.type).toBe('image/png');
    expect(new Uint8Array(await submitted.arrayBuffer())).toEqual(
      new Uint8Array([1, 2, 3]),
    );
  });

  it.each(['', '/daemon/', 'http://127.0.0.1:8080/prefix/'])(
    'downloads binary data through the injected fetch with base URL %s',
    async (baseUrl) => {
      const fetch = vi.fn(
        async () =>
          new Response(new Uint8Array([4, 5, 6]), {
            headers: { 'content-type': 'image/png' },
          }),
      );
      const client = createDaemonClient({ baseUrl, fetch });
      const signal = new AbortController().signal;
      const downloaded = await client.attachments.download(
        'agent/a',
        'att:1?',
        { signal },
      );
      expect(downloaded).toBeInstanceOf(Blob);
      expect(downloaded.type).toBe('image/png');
      expect(new Uint8Array(await downloaded.arrayBuffer())).toEqual(
        new Uint8Array([4, 5, 6]),
      );
      const [url, init] = fetch.mock.calls[0] as unknown as [
        string,
        RequestInit,
      ];
      expect(url).toBe(
        `${baseUrl.replace(/\/+$/, '')}/api/agents/agent%2Fa/attachments/att%3A1%3F`,
      );
      expect(init.signal).toBe(signal);
      expect(new Headers(init.headers).has('content-type')).toBe(false);
    },
  );

  it('surfaces upload HTTP errors through the existing JSON helper', async () => {
    const client = createDaemonClient({
      fetch: async () => Response.json({ error: 'Too large' }, { status: 413 }),
    });
    const upload = client.attachments.upload(
      'agent',
      'session',
      new Blob(['text']),
      'brief.txt',
    );
    await expect(upload).rejects.toBeInstanceOf(DaemonHttpError);
    await expect(upload).rejects.toMatchObject({
      status: 413,
      body: { error: 'Too large' },
    });
  });

  it.each(['upload', 'download'] as const)(
    'preserves fetch aborts for %s',
    async (operation) => {
      const abort = new DOMException('Aborted', 'AbortError');
      const signal = new AbortController().signal;
      const fetch = vi.fn(async (_url, init) => {
        expect(init?.signal).toBe(signal);
        throw abort;
      });
      const client = createDaemonClient({ fetch });
      const request =
        operation === 'upload'
          ? client.attachments.upload(
              'agent',
              'session',
              new Blob(['text']),
              'brief.txt',
              { signal },
            )
          : client.attachments.download('agent', 'attachment', { signal });
      await expect(request).rejects.toBe(abort);
    },
  );
});

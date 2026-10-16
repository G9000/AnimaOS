import { expect, it, vi } from 'vitest';
import { createDaemonClient } from './index.js';

it('lists workspace files and encodes relative paths for text preview', async () => {
  const urls: string[] = [];
  const client = createDaemonClient({
    baseUrl: '',
    fetch: async (url) => {
      urls.push(String(url));
      return Response.json(
        String(url).includes('/files')
          ? {
              files: [
                {
                  path: 'notes/a #1.md',
                  name: 'a #1.md',
                  sizeBytes: 12,
                  modifiedAtMs: null,
                },
              ],
              truncated: true,
            }
          : { path: 'notes/a #1.md', content: 'Hello', truncated: false },
      );
    },
  });
  expect(await client.workspace.listFiles()).toMatchObject({
    truncated: true,
    files: [{ modifiedAtMs: null }],
  });
  expect(await client.workspace.readFile('notes/a #1.md')).toMatchObject({
    content: 'Hello',
  });
  expect(urls).toEqual([
    '/api/workspace/files',
    '/api/workspace/file?path=notes%2Fa%20%231.md',
  ]);
});

it('m68_downloads_exact_report_bytes_through_the_authenticated_encoded_get', async () => {
  const bytes = new TextEncoder().encode(
    '\uFEFFname,value\r\n"café 🦀",137.42\r\n',
  );
  const controller = new AbortController();
  const received = vi.fn(
    async (_url: Parameters<typeof fetch>[0], _init?: RequestInit) =>
      new Response(bytes, {
        headers: {
          'content-type': 'application/octet-stream',
          'content-length': String(bytes.byteLength),
        },
      }),
  );
  const client = createDaemonClient({
    baseUrl: '/daemon',
    // Owner authentication belongs to the configured transport, as it does
    // for other SDK methods. This synthetic header is never a real credential.
    fetch: async (url, init) => {
      const headers = new Headers(init?.headers);
      headers.set('authorization', 'Bearer synthetic-owner');
      return received(url, { ...init, headers });
    },
  });
  const path = 'reports/résumé #1% +.CSV';
  const blob = await client.workspace.downloadFile(path, {
    signal: controller.signal,
  });

  expect(Array.from(new Uint8Array(await blob.arrayBuffer()))).toEqual(
    Array.from(bytes),
  );
  expect(blob.type).toBe('application/octet-stream');
  expect(received).toHaveBeenCalledTimes(1);
  const [url, init] = received.mock.calls[0]!;
  expect(url).toBe(
    `/daemon/api/workspace/file/download?path=${encodeURIComponent(path)}`,
  );
  expect(init?.method ?? 'GET').toBe('GET');
  expect(init?.signal).toBe(controller.signal);
  expect(new Headers(init?.headers).get('authorization')).toBe(
    'Bearer synthetic-owner',
  );
  expect(new Headers(init?.headers).get('accept')).toBe(
    'application/octet-stream',
  );
  expect(init?.body).toBeUndefined();
});

it.each([0, 1024 * 1024])(
  'm68_preserves_report_size_at_the_empty_and_1MiB_bounds_%s',
  async (size) => {
    const bytes = new Uint8Array(size).fill(120);
    const fetch = vi.fn(
      async () =>
        new Response(bytes, {
          headers: {
            'content-type': 'application/octet-stream',
            'content-length': String(size),
          },
        }),
    );
    const client = createDaemonClient({ baseUrl: '', fetch });
    const blob = await client.workspace.downloadFile('reports/result.txt');
    expect(blob.size).toBe(size);
    expect(
      new Uint8Array(await blob.arrayBuffer()).every((byte) => byte === 120),
    ).toBe(true);
    expect(fetch).toHaveBeenCalledTimes(1);
  },
);

it.each([
  {
    name: 'declared oversize',
    size: 1024 * 1024 + 1,
    type: 'application/octet-stream',
    status: 200,
  },
  { name: 'wrong type', size: 2, type: 'application/json', status: 200 },
  { name: 'owner denial', size: 2, type: 'application/json', status: 403 },
])(
  'm68_refuses $name instead of saving a blob',
  async ({ size, type, status }) => {
    const fetch = vi.fn(
      async () =>
        new Response('{}', {
          status,
          headers: { 'content-type': type, 'content-length': String(size) },
        }),
    );
    const client = createDaemonClient({ baseUrl: '', fetch });
    await expect(
      client.workspace.downloadFile('reports/result.csv'),
    ).rejects.toThrow();
    expect(fetch).toHaveBeenCalledTimes(1);
  },
);

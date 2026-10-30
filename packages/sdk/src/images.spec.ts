import { describe, expect, it, vi } from 'vitest';
import { DaemonClient } from './client.js';

const attachment = { id: 'image-1', name: 'image.png', mime: 'image/png', size: 128, kind: 'image', path: 'uploads/image-1/image.png', createdAtMs: 10 };
const version = { id: 'version-1', attachment, parentAttachmentId: null, width: 1024, height: 1024, model: 'gpt-image-2.5-sunburst', createdAtMs: 10 };
const operation = { id: 'operation-1', prompt: 'A blue bird', sourceAttachmentId: null, createdAtMs: 9, status: 'succeeded', version };
const page = () => ({ clearGeneration: 2, provider: 'openai', model: 'gpt-image-2.5-sunburst', operations: [operation] });
function client(value: unknown, headers: Record<string, string> = {}) {
  const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify(value), { headers: { 'content-type': 'application/json', ...headers } }));
  return { sdk: new DaemonClient({ baseUrl: '', ownerToken: 'owner', fetch }), fetch };
}

describe('image records', () => {
  it('reads only the encoded owner session route with bounded JSON and cancellation', async () => {
    const { sdk, fetch } = client(page());
    const signal = new AbortController().signal;
    expect(await sdk.images.list('agent/a', 'chat:one', { signal })).toEqual(page());
    expect(fetch).toHaveBeenCalledWith('/api/agents/agent%2Fa/sessions/chat%3Aone/images', expect.objectContaining({ signal }));
    expect(new Headers(fetch.mock.calls[0][1].headers).get('authorization')).toBe('Bearer owner');
    expect(new Headers(fetch.mock.calls[0][1].headers).get('accept')).toBe('application/json');
    expect(Object.getOwnPropertyNames(Object.getPrototypeOf(sdk.images)).sort()).toEqual(['constructor', 'list']);
  });

  it.each([
    () => ({ ...page(), url: 'https://untrusted.test/image.png' }),
    () => ({ ...page(), operations: Array.from({ length: 33 }, (_, index) => ({ ...operation, id: `operation-${index}` })) }),
    () => ({ ...page(), operations: [operation, operation] }),
    () => ({ ...page(), operations: [{ ...operation, status: 'failed' }] }),
    () => ({ ...page(), operations: [{ ...operation, status: { toString: 'succeeded' } }] }),
    () => ({ ...page(), operations: [{ ...operation, version: { ...version, width: 2048 } }] }),
    () => ({ ...page(), operations: [{ ...operation, version: { ...version, attachment: { ...attachment, mime: 'text/html' } } }] }),
    () => ({ ...page(), operations: [{ ...operation, sourceAttachmentId: 'source', version }] }),
  ])('rejects inconsistent or open image records', async (invalid) => {
    await expect(client(invalid()).sdk.images.list('a', 's')).rejects.toThrow('Invalid image response');
  });

  it('cancels oversized responses before decoding them', async () => {
    await expect(client(page(), { 'content-length': String(512 * 1024 + 1) }).sdk.images.list('a', 's')).rejects.toThrow('Invalid bounded response length');
  });
});

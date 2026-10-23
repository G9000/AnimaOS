import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { clearOwnerApiKey, setOwnerApiKey } from '../lib/owner-api-key';
import { useDaemonImage } from './useDaemonImage';

const createObjectURL = vi.fn(() => 'blob:daemon-avatar');
const revokeObjectURL = vi.fn();
beforeEach(() => {
  clearOwnerApiKey(); createObjectURL.mockClear(); revokeObjectURL.mockClear();
  Object.defineProperties(URL, { createObjectURL: { configurable: true, value: createObjectURL }, revokeObjectURL: { configurable: true, value: revokeObjectURL } });
});
afterEach(() => { clearOwnerApiKey(); vi.unstubAllGlobals(); });

describe('daemon image authentication', () => {
  it('preserves the direct URL and starts no fetch without an owner key', () => {
    const fetch = vi.fn(); vi.stubGlobal('fetch', fetch);
    const { result } = renderHook(() => useDaemonImage('/api/workspace/avatar?v=0'));
    expect(result.current.src).toBe('/api/workspace/avatar?v=0');
    expect(fetch).not.toHaveBeenCalled();
  });
  it('loads a bounded image with header authentication and revokes it on unmount', async () => {
    setOwnerApiKey('owner-secret');
    const fetch = vi.fn().mockResolvedValue(new Response(new Uint8Array([1, 2, 3]), { headers: { 'content-type': 'image/png' } }));
    vi.stubGlobal('fetch', fetch);
    const { result, unmount } = renderHook(() => useDaemonImage('/api/agents/nova/avatar?v=0'));
    await waitFor(() => expect(result.current.src).toBe('blob:daemon-avatar'));
    expect(fetch.mock.calls[0][0]).toBe('/api/agents/nova/avatar?v=0');
    expect(fetch.mock.calls[0][1]).toMatchObject({ headers: { 'x-api-key': 'owner-secret' }, redirect: 'error', credentials: 'same-origin', cache: 'no-store', signal: expect.any(AbortSignal) });
    expect(String(fetch.mock.calls[0][0])).not.toContain('owner-secret');
    unmount();
    expect(fetch.mock.calls[0][1].signal.aborted).toBe(true);
    expect(revokeObjectURL).toHaveBeenCalledWith('blob:daemon-avatar');
  });
  it('rejects external destinations and oversized bodies without making image URLs', async () => {
    setOwnerApiKey('owner-secret'); const fetch = vi.fn(); vi.stubGlobal('fetch', fetch);
    const { result, rerender } = renderHook(({ source }) => useDaemonImage(source), { initialProps: { source: 'https://outside.example/api/avatar' } });
    await waitFor(() => expect(result.current.failed).toBe(true));
    expect(fetch).not.toHaveBeenCalled();
    fetch.mockResolvedValue(new Response(new Uint8Array([1]), { headers: { 'content-type': 'image/png', 'content-length': String(5 * 1024 * 1024 + 1) } }));
    rerender({ source: '/api/workspace/avatar?v=1' });
    await waitFor(() => expect(result.current.failed).toBe(true));
    expect(createObjectURL).not.toHaveBeenCalled();
  });
  it('aborts an old scope and ignores its response after another image is selected', async () => {
    setOwnerApiKey('owner-secret');
    let oldResponse!: (response: Response) => void;
    const fetch = vi.fn().mockImplementationOnce(() => new Promise<Response>((resolve) => { oldResponse = resolve; })).mockResolvedValueOnce(new Response(new Uint8Array([4]), { headers: { 'content-type': 'image/webp' } }));
    vi.stubGlobal('fetch', fetch);
    const { result, rerender } = renderHook(({ source }) => useDaemonImage(source), { initialProps: { source: '/api/agents/old/avatar' } });
    rerender({ source: '/api/agents/new/avatar' });
    expect(fetch.mock.calls[0][1].signal.aborted).toBe(true);
    await waitFor(() => expect(result.current.src).toBe('blob:daemon-avatar'));
    await act(async () => { oldResponse(new Response(new Uint8Array([1]), { headers: { 'content-type': 'image/png' } })); });
    expect(createObjectURL).toHaveBeenCalledTimes(1);
    expect(result.current.src).toBe('blob:daemon-avatar');
  });
  it('enforces the streaming byte limit even without a content length', async () => {
    setOwnerApiKey('owner-secret');
    const cancel = vi.fn();
    const body = new ReadableStream({ start(controller) { controller.enqueue(new Uint8Array(5 * 1024 * 1024 + 1)); }, cancel });
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(body, { headers: { 'content-type': 'image/png' } })));
    const { result } = renderHook(() => useDaemonImage('/api/workspace/avatar'));
    await waitFor(() => expect(result.current.failed).toBe(true));
    expect(cancel).toHaveBeenCalled();
    expect(createObjectURL).not.toHaveBeenCalled();
  });
});

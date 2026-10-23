import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import * as transport from '../lib/attachments';
import { useAttachments } from './useAttachments';

const attachment = {
  id: 'att_1',
  name: 'photo.png',
  mime: 'image/png',
  size: 3,
  kind: 'image' as const,
  path: 'attachments/photo.png',
  createdAtMs: 1,
};
const file = () => new File(['png'], 'photo.png', { type: 'image/png' });
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
const options = () => ({
  chatKey: 'first',
  agentId: 'agent',
  sessionId: 'session' as string | null,
  ensureSession: vi.fn(async () => 'created'),
  enabled: true,
});
beforeEach(() => {
  vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:local');
  vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
});
afterEach(() => vi.restoreAllMocks());

it('reserves the cap synchronously and keeps upload progress pending until success', async () => {
  const pending = deferred<typeof attachment>();
  const upload = vi
    .spyOn(transport, 'uploadAttachmentWithProgress')
    .mockReturnValue(pending.promise);
  const { result } = renderHook(() => useAttachments(options()));
  act(() => {
    result.current.add(Array.from({ length: 8 }, file));
    result.current.add(Array.from({ length: 8 }, file));
  });
  expect(result.current.items).toHaveLength(10);
  expect(result.current.error).toBe(transport.ATTACHMENT_ERRORS.limit);
  await act(async () => {
    await Promise.resolve();
  });
  act(() => upload.mock.calls[0][3]?.onProgress?.(100));
  expect(result.current.uploading).toBe(true);
  expect(result.current.readyAttachments).toHaveLength(0);
  await act(async () => pending.resolve(attachment));
  expect(result.current.uploading).toBe(false);
  expect(result.current.items.every((item) => item.status === 'ready')).toBe(
    true,
  );
});

it.each(['switch', 'disabled', 'remove', 'unmount'] as const)(
  'aborts on %s, revokes local previews and ignores late completion',
  async (boundary) => {
    const pending = deferred<typeof attachment>();
    const upload = vi
      .spyOn(transport, 'uploadAttachmentWithProgress')
      .mockReturnValue(pending.promise);
    const props = options();
    const { result, rerender, unmount } = renderHook(useAttachments, {
      initialProps: props,
    });
    act(() => result.current.add([file()]));
    await act(async () => {
      await Promise.resolve();
    });
    const signal = upload.mock.calls[0][3]?.signal;
    if (boundary === 'switch')
      rerender({ ...props, chatKey: 'second', sessionId: 'second-session' });
    if (boundary === 'disabled') rerender({ ...props, enabled: false });
    if (boundary === 'remove')
      act(() => result.current.remove(result.current.items[0].localId));
    if (boundary === 'unmount') unmount();
    expect(signal?.aborted).toBe(true);
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:local');
    await act(async () => pending.resolve(attachment));
    expect(result.current.readyAttachments).toHaveLength(0);
  },
);

it('retains ready metadata for the original chat while revoking previews on switch', async () => {
  vi.spyOn(transport, 'uploadAttachmentWithProgress').mockResolvedValue(
    attachment,
  );
  const props = options();
  const { result, rerender } = renderHook(useAttachments, {
    initialProps: props,
  });
  await act(async () => result.current.add([file()]));
  rerender({ ...props, chatKey: 'second' });
  expect(result.current.items).toHaveLength(0);
  rerender(props);
  expect(result.current.readyAttachments).toEqual([attachment]);
  expect(result.current.items[0].previewUrl).toBeUndefined();
});

it('shares one new-session request per file batch and ignores removal before creation resolves', async () => {
  const created = deferred<string>();
  const props = {
    ...options(),
    sessionId: null,
    ensureSession: vi.fn(() => created.promise),
  };
  const upload = vi
    .spyOn(transport, 'uploadAttachmentWithProgress')
    .mockResolvedValue(attachment);
  const { result } = renderHook(useAttachments, { initialProps: props });
  act(() => result.current.add([file(), file()]));
  expect(props.ensureSession).toHaveBeenCalledOnce();
  act(() => result.current.remove(result.current.items[0].localId));
  await act(async () => created.resolve('created'));
  expect(upload).toHaveBeenCalledOnce();
  expect(upload.mock.calls[0].slice(0, 2)).toEqual(['agent', 'created']);
});

it('shows invalid-file and upload failures safely, restores metadata only and blocks disabled adds', async () => {
  vi.spyOn(transport, 'uploadAttachmentWithProgress').mockRejectedValue(
    new Error('/private/path token=secret'),
  );
  const props = options();
  const { result, rerender } = renderHook(useAttachments, {
    initialProps: props,
  });
  await act(async () =>
    result.current.add([new File([], 'empty.txt'), file()]),
  );
  expect(result.current.items.map((item) => item.error)).toEqual([
    transport.ATTACHMENT_ERRORS.empty,
    transport.ATTACHMENT_ERRORS.upload,
  ]);
  act(() => {
    result.current.clear();
    result.current.restore([
      { ...attachment, data: 'secret', previewUrl: 'blob:old' },
    ]);
  });
  expect(result.current.readyAttachments).toEqual([attachment]);
  expect(result.current.items[0].previewUrl).toBeUndefined();
  rerender({ ...props, enabled: false });
  act(() => result.current.add([file()]));
  expect(result.current.items).toHaveLength(1);
});

it('keeps removable ID-only recovery references without inventing public file metadata', () => {
  const { result } = renderHook(() => useAttachments(options()));
  act(() => result.current.restore([], ['saved_2', 'saved_1']));
  expect(result.current.readyAttachmentIds).toEqual(['saved_2', 'saved_1']);
  expect(result.current.readyAttachments).toEqual([]);
  expect(result.current.items).toHaveLength(2);
  act(() => result.current.remove(result.current.items[0].localId));
  expect(result.current.readyAttachmentIds).toEqual(['saved_1']);
});

it('restores atomically or leaves a nearly-full draft unchanged', () => {
  const { result } = renderHook(() => useAttachments(options()));
  act(() =>
    result.current.restore(
      [],
      Array.from({ length: 9 }, (_, i) => `existing_${i}`),
    ),
  );
  const before = result.current.items;
  act(() => {
    expect(
      result.current.restore(
        [],
        Array.from({ length: 10 }, (_, i) => `recovered_${i}`),
      ),
    ).toBe(false);
  });
  expect(result.current.items).toBe(before);
  expect(result.current.readyAttachmentIds).toEqual(
    Array.from({ length: 9 }, (_, i) => `existing_${i}`),
  );
  expect(result.current.error).toBe(transport.ATTACHMENT_ERRORS.limit);
  act(() => result.current.clear());
  act(() => {
    expect(
      result.current.restore(
        [],
        Array.from({ length: 10 }, (_, i) => `recovered_${i}`),
      ),
    ).toBe(true);
  });
  expect(result.current.readyAttachmentIds).toEqual(
    Array.from({ length: 10 }, (_, i) => `recovered_${i}`),
  );
});

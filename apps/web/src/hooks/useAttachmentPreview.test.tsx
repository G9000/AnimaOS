import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { daemon } from '../lib/daemon-api';
import { useAttachmentPreview } from './useAttachmentPreview';
let resolve!: (blob: Blob) => void;
beforeEach(() => {
  vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:preview');
  vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
});
afterEach(() => vi.restoreAllMocks());
it('uses credential-carrying SDK Blob transport and revokes on identity change/unmount', async () => {
  const download = vi
    .spyOn(daemon, 'downloadAttachment')
    .mockResolvedValue(new Blob(['png'], { type: 'image/png' }));
  const { result, rerender, unmount } = renderHook(
    ({ id }) => useAttachmentPreview('agent', id, 'image/png', true),
    { initialProps: { id: 'first' } },
  );
  await act(async () => {
    await Promise.resolve();
  });
  expect(download).toHaveBeenCalledWith('agent', 'first', {
    signal: expect.any(AbortSignal),
  });
  expect(result.current.url).toBe('blob:preview');
  expect(URL.revokeObjectURL).not.toHaveBeenCalled();
  rerender({ id: 'second' });
  expect(URL.revokeObjectURL).toHaveBeenCalledOnce();
  await act(async () => {
    await Promise.resolve();
  });
  unmount();
  expect(URL.revokeObjectURL).toHaveBeenCalledTimes(2);
});
it('ignores stale download completion and aborts when disabled', async () => {
  const download = vi.spyOn(daemon, 'downloadAttachment').mockImplementation(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  const { result, rerender } = renderHook(
    (enabled) =>
      useAttachmentPreview('agent', 'attachment', 'image/png', enabled),
    { initialProps: true },
  );
  const signal = download.mock.calls[0][2]?.signal;
  rerender(false);
  expect(signal?.aborted).toBe(true);
  await act(async () => resolve(new Blob(['png'])));
  expect(result.current.url).toBeNull();
  expect(URL.createObjectURL).not.toHaveBeenCalled();
});
it('keeps preview failures safe, supports retry and refuses unsafe image types', async () => {
  const download = vi
    .spyOn(daemon, 'downloadAttachment')
    .mockRejectedValueOnce(new Error('token=secret /private/path'))
    .mockResolvedValue(new Blob(['png']));
  const { result, rerender } = renderHook(
    (mime) => useAttachmentPreview('agent', 'attachment', mime, true),
    { initialProps: 'image/svg+xml' },
  );
  expect(download).not.toHaveBeenCalled();
  rerender('image/png');
  await act(async () => {
    await Promise.resolve();
  });
  expect(result.current.error).toBe(
    'The image preview is unavailable. Retry or download the file.',
  );
  await act(async () => result.current.retry());
  expect(result.current.url).toBe('blob:preview');
  act(() => result.current.imageFailed());
  expect(result.current.url).toBeNull();
  expect(URL.revokeObjectURL).toHaveBeenCalledOnce();
});

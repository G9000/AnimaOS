import { act, renderHook } from '@testing-library/react';
import { StatusTooOldError } from '@animaOS-SWARM/sdk';
import { afterEach, expect, it, vi } from 'vitest';
import { daemon } from '../lib/daemon-api';
import { statusFixture } from '../test/system';
import { deferred } from '../test/view-harness';
import { useDaemonCompatibility } from './useDaemonCompatibility';

afterEach(() => vi.restoreAllMocks());
it('404 blocks compatibility, a newer epoch clears it, and no polling occurs', async () => {
  vi.spyOn(daemon, 'status')
    .mockRejectedValueOnce(new StatusTooOldError())
    .mockResolvedValue(statusFixture());
  const { result, rerender } = renderHook(useDaemonCompatibility, {
    initialProps: { enabled: true, epoch: 0 },
  });
  await act(async () => {});
  expect(result.current.statusUnsupported).toBe(true);
  expect(daemon.status).toHaveBeenCalledTimes(1);
  rerender({ enabled: true, epoch: 1 });
  await act(async () => {});
  expect(result.current.statusUnsupported).toBe(false);
  expect(daemon.status).toHaveBeenCalledTimes(2);
});
it.each([401, 403, 500, null])(
  'does not label status %s as an older daemon',
  async (status) => {
    vi.spyOn(daemon, 'status').mockRejectedValue(
      Object.assign(new Error('unavailable'), { status }),
    );
    const { result } = renderHook(() =>
      useDaemonCompatibility({ enabled: true, epoch: 1 }),
    );
    await act(async () => {});
    expect(result.current.statusUnsupported).toBe(false);
  },
);
it('ignores stale and disabled probes', async () => {
  const pending = deferred<Awaited<ReturnType<typeof daemon.status>>>();
  vi.spyOn(daemon, 'status')
    .mockReturnValueOnce(pending.promise)
    .mockResolvedValue(statusFixture());
  const { result, rerender, unmount } = renderHook(useDaemonCompatibility, {
    initialProps: { enabled: true, epoch: 0 },
  });
  rerender({ enabled: false, epoch: 1 });
  await act(async () => pending.reject(new StatusTooOldError()));
  expect(result.current.statusUnsupported).toBe(false);
  expect(daemon.status).toHaveBeenCalledTimes(1);
  unmount();
});

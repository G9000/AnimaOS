import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  daemon,
  type DriveStatus,
  type DriveConnectResult,
} from '../lib/daemon-api';
import { driveFixture } from '../test/drive';
import {
  DRIVE_CONSENT_POLL_MS,
  useDriveIntegrations,
} from './useDriveIntegrations';

vi.mock('../lib/daemon-api', async () => {
  const actual =
    await vi.importActual<typeof import('../lib/daemon-api')>(
      '../lib/daemon-api',
    );
  return {
    ...actual,
    daemon: {
      ...actual.daemon,
      driveStatus: vi.fn(),
      connectDrive: vi.fn(),
      disconnectDrive: vi.fn(),
    },
  };
});
beforeEach(() => {
  vi.mocked(daemon.driveStatus)
    .mockReset()
    .mockResolvedValue({ configured: true, connector: null });
  vi.mocked(daemon.connectDrive).mockReset();
  vi.mocked(daemon.disconnectDrive).mockReset();
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});
describe('useDriveIntegrations', () => {
  it('fp02 offline and epoch fence Drive reads and settled actions without refetch or consent publication', async () => {
    let finish!: (value: DriveConnectResult) => void;
    const { result, rerender } = renderHook(
      ({ online, epoch }) => useDriveIntegrations('agent-a', online, epoch),
      { initialProps: { online: false, epoch: 1 } },
    );
    await act(async () => {
      await result.current.refresh();
    });
    expect(daemon.driveStatus).not.toHaveBeenCalled();
    rerender({ online: true, epoch: 2 });
    await waitFor(() => expect(result.current.available).toBe(true));
    vi.mocked(daemon.connectDrive).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    let action!: Promise<boolean>;
    act(() => {
      action = result.current.connect({ selectedFileIds: ['File_1'] });
    });
    rerender({ online: false, epoch: 2 });
    await act(async () => {
      finish({
        connector: driveFixture({ status: 'pairing' }),
        consentUrl: 'https://accounts.google.com/o/oauth2/v2/auth?state=old',
      });
      expect(await action).toBe(false);
    });
    expect(result.current.consentUrl).toBeNull();
    expect(result.current.status).toBeNull();
    expect(daemon.driveStatus).toHaveBeenCalledTimes(1);
    expect(daemon.connectDrive).toHaveBeenCalledTimes(1);
  });

  it('discards prior agent reads/actions and clears pending consent URLs on agent switch', async () => {
    let finish!: (value: DriveConnectResult) => void;
    const { result, rerender } = renderHook(
      ({ id }) => useDriveIntegrations(id),
      { initialProps: { id: 'agent-a' } },
    );
    await waitFor(() => expect(result.current.loading).toBe(false));
    const old = result.current;
    vi.mocked(daemon.connectDrive).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    act(() => {
      void result.current.connect({ selectedFileIds: ['File_1'] });
    });
    rerender({ id: 'agent-b' });
    await act(async () => {
      finish({
        connector: driveFixture({ status: 'pairing' }),
        consentUrl: 'https://accounts.google.com/o/oauth2/v2/auth?state=old',
      });
    });
    expect(result.current.consentUrl).toBeNull();
    expect(result.current.status?.connector).toBeNull();
    vi.mocked(daemon.connectDrive).mockClear();
    await act(async () => {
      expect(await old.connect({ selectedFileIds: ['File_1'] })).toBe(false);
      await old.refresh();
    });
    expect(daemon.connectDrive).not.toHaveBeenCalled();
  });
  it('rejects hostile consent URLs and refreshes actual saved state after errors without exposing raw details', async () => {
    vi.mocked(daemon.connectDrive).mockResolvedValue({
      connector: driveFixture({ status: 'pairing' }),
      consentUrl:
        'https://accounts.google.com/Logout?continue=https://evil.test',
    });
    const { result } = renderHook(() => useDriveIntegrations('agent-a'));
    await waitFor(() => expect(result.current.loading).toBe(false));
    await act(async () => {
      expect(
        await result.current.connect({ selectedFileIds: ['File_1'] }),
      ).toBe(false);
    });
    expect(result.current.consentUrl).toBeNull();
    expect(result.current.error).toBe(
      'Google returned an invalid consent link. Refresh Drive status before trying again.',
    );
  });
  it('refreshes locally revoked cleanup state after a failed disconnect and preserves its safe error', async () => {
    const c = driveFixture();
    vi.mocked(daemon.driveStatus).mockResolvedValue({
      configured: true,
      connector: c,
    });
    vi.mocked(daemon.disconnectDrive).mockRejectedValue(
      new Error('private upstream response'),
    );
    const { result } = renderHook(() => useDriveIntegrations('agent-a'));
    await waitFor(() => expect(result.current.status?.connector).toEqual(c));
    vi.mocked(daemon.driveStatus).mockResolvedValue({
      configured: true,
      connector: { ...c, status: 'disconnected', cleanupPending: true },
    });
    await act(async () => {
      expect(await result.current.disconnect(c.id)).toBe(false);
    });
    expect(result.current.status?.connector?.cleanupPending).toBe(true);
    expect(result.current.error).not.toContain('private');
  });
  it('polls pending consent only while mounted and stops when the connection activates', async () => {
    vi.useFakeTimers();
    vi.spyOn(document, 'visibilityState', 'get').mockReturnValue('visible');
    vi.mocked(daemon.driveStatus).mockResolvedValue({
      configured: true,
      connector: driveFixture({ status: 'pairing' }),
    });
    const { result, unmount } = renderHook(() =>
      useDriveIntegrations('agent-a'),
    );
    await act(async () => {});
    vi.mocked(daemon.driveStatus).mockResolvedValue({
      configured: true,
      connector: driveFixture(),
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(DRIVE_CONSENT_POLL_MS);
    });
    expect(result.current.status?.connector?.status).toBe('active');
    const calls = vi.mocked(daemon.driveStatus).mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(DRIVE_CONSENT_POLL_MS * 3);
    });
    expect(daemon.driveStatus).toHaveBeenCalledTimes(calls);
    unmount();
  });
  it('discards stale status after a switch and fails closed on a status read error', async () => {
    let finish!: (value: DriveStatus) => void;
    vi.mocked(daemon.driveStatus).mockReturnValueOnce(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const { result, rerender } = renderHook(
      ({ id }) => useDriveIntegrations(id),
      { initialProps: { id: 'agent-a' } },
    );
    rerender({ id: 'agent-b' });
    await act(async () => {
      finish({ configured: true, connector: driveFixture() });
    });
    expect(result.current.status?.connector).toBeNull();
    vi.mocked(daemon.driveStatus).mockRejectedValue(new Error('secret'));
    await act(async () => {
      await result.current.refresh();
    });
    expect(result.current.available).toBe(false);
    expect(result.current.error).not.toContain('secret');
  });

  it('stops pending-consent polling after forty attempts while retaining explicit refresh', async () => {
    vi.useFakeTimers();
    vi.spyOn(document, 'visibilityState', 'get').mockReturnValue('visible');
    vi.mocked(daemon.driveStatus).mockResolvedValue({
      configured: true,
      connector: driveFixture({ status: 'pairing' }),
    });
    const { result, unmount } = renderHook(() =>
      useDriveIntegrations('agent-a'),
    );
    await act(async () => {});
    expect(daemon.driveStatus).toHaveBeenCalledTimes(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(DRIVE_CONSENT_POLL_MS * 45);
    });
    expect(daemon.driveStatus).toHaveBeenCalledTimes(41);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(DRIVE_CONSENT_POLL_MS * 5);
    });
    expect(daemon.driveStatus).toHaveBeenCalledTimes(41);
    await act(async () => {
      await result.current.refresh();
    });
    expect(daemon.driveStatus).toHaveBeenCalledTimes(42);
    unmount();
  });
});

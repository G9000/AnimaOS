import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { daemon } from '../lib/daemon-api';
import { discordFixture } from '../test/discord';
import {
  DISCORD_STATUS_POLL_MS,
  useDiscordIntegrations,
} from './useDiscordIntegrations';

vi.mock('../lib/daemon-api', async () => {
  const actual =
    await vi.importActual<typeof import('../lib/daemon-api')>(
      '../lib/daemon-api',
    );
  return {
    ...actual,
    daemon: {
      ...actual.daemon,
      discordConnectors: vi.fn(),
      createDiscordConnector: vi.fn(),
      activateDiscordConnector: vi.fn(),
      suppressDiscordUnknownDelivery: vi.fn(),
      replaceDiscordCredential: vi.fn(),
      deleteDiscordConnector: vi.fn(),
    },
  };
});

beforeEach(() => {
  vi.mocked(daemon.discordConnectors).mockReset().mockResolvedValue([]);
  vi.mocked(daemon.createDiscordConnector).mockReset();
  vi.mocked(daemon.activateDiscordConnector).mockReset();
  vi.mocked(daemon.suppressDiscordUnknownDelivery).mockReset();
  vi.mocked(daemon.replaceDiscordCredential).mockReset();
  vi.mocked(daemon.deleteDiscordConnector).mockReset();
});
afterEach(() => {
  vi.useRealTimers();
});
describe('useDiscordIntegrations', () => {
  it('fp02 Discord suppresses offline reads and fences old epoch mutations', async () => {
    const { result, rerender } = renderHook(
      ({ online, epoch }) =>
        useDiscordIntegrations('agent-a', true, online, epoch),
      { initialProps: { online: false, epoch: 1 } },
    );
    await act(async () => {
      await result.current.refresh();
    });
    expect(daemon.discordConnectors).not.toHaveBeenCalled();
    rerender({ online: true, epoch: 2 });
    await waitFor(() => expect(result.current.available).toBe(true));
    const old = result.current;
    rerender({ online: true, epoch: 3 });
    await act(async () => {
      expect(await old.activate('connector-1')).toBe(false);
    });
    expect(daemon.activateDiscordConnector).not.toHaveBeenCalled();
  });

  it('discards stale agent load and create results', async () => {
    let load!: (value: ReturnType<typeof discordFixture>[]) => void;
    let create!: (value: ReturnType<typeof discordFixture>) => void;
    vi.mocked(daemon.discordConnectors).mockReturnValueOnce(
      new Promise((resolve) => {
        load = resolve;
      }),
    );
    vi.mocked(daemon.createDiscordConnector).mockReturnValue(
      new Promise((resolve) => {
        create = resolve;
      }),
    );
    const { result, rerender } = renderHook(
      ({ id }) => useDiscordIntegrations(id),
      { initialProps: { id: 'agent-a' } },
    );
    act(() => {
      void result.current.create({
        botToken: 'secret',
        guildId: '3',
        channelId: '2',
        approvedUserIds: ['4'],
        privateChannelConfirmed: true,
      });
    });
    rerender({ id: 'agent-b' });
    await act(async () => {
      load([discordFixture()]);
      create(discordFixture());
    });
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.connectors).toEqual([]);
    expect(result.current.busy).toBeNull();
  });
  it('keeps saved setup disabled until activation and replaces uncertain state after suppression', async () => {
    const c = discordFixture();
    vi.mocked(daemon.discordConnectors).mockResolvedValue([c]);
    vi.mocked(daemon.activateDiscordConnector).mockResolvedValue({
      ...c,
      enabled: true,
      status: 'ready',
    });
    vi.mocked(daemon.suppressDiscordUnknownDelivery).mockResolvedValue({
      ...c,
      enabled: true,
      status: 'ready',
      unknownOutbound: 0,
    });
    const { result } = renderHook(() => useDiscordIntegrations('agent-a'));
    await waitFor(() => expect(result.current.connectors).toEqual([c]));
    vi.mocked(daemon.discordConnectors).mockResolvedValue([
      { ...c, enabled: true, status: 'ready' },
    ]);
    await act(async () => {
      expect(await result.current.activate(c.id)).toBe(true);
    });
    expect(result.current.connectors[0].status).toBe('ready');
    await act(async () => {
      await result.current.suppress(c.id);
    });
    expect(daemon.suppressDiscordUnknownDelivery).toHaveBeenCalledWith(
      'agent-a',
      c.id,
    );
  });
  it('never displays raw provider errors or echoed bot tokens', async () => {
    vi.mocked(daemon.discordConnectors).mockRejectedValue(
      new Error('token-secret upstream body'),
    );
    const { result } = renderHook(() => useDiscordIntegrations('agent-a'));
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.error).toBe(
      'Discord request failed. Refresh status and check the connection setup.',
    );
  });
  it('retains saved setup after replacement and delete failures with safe actionable errors', async () => {
    const c = discordFixture();
    vi.mocked(daemon.discordConnectors).mockResolvedValue([c]);
    vi.mocked(daemon.replaceDiscordCredential).mockRejectedValue({
      status: 409,
      message: 'raw secret',
    });
    vi.mocked(daemon.deleteDiscordConnector).mockRejectedValue(
      new Error('raw secret'),
    );
    const { result } = renderHook(() => useDiscordIntegrations('agent-a'));
    await waitFor(() => expect(result.current.connectors).toEqual([c]));
    await act(async () => {
      expect(await result.current.replace(c.id, 'secret')).toBe(false);
    });
    expect(result.current.error).toContain('same bot');
    await act(async () => {
      expect(await result.current.disconnect(c.id)).toBe(false);
    });
    expect(result.current.connectors).toEqual([c]);
    expect(result.current.error).not.toContain('secret');
  });

  it('polls active surfaces without overlapping reads and discovers uncertain delivery, then cleans up', async () => {
    vi.useFakeTimers();
    const c = discordFixture({ enabled: true, status: 'ready' });
    vi.mocked(daemon.discordConnectors).mockResolvedValue([c]);
    const { result, unmount } = renderHook(() =>
      useDiscordIntegrations('agent-a', true),
    );
    await act(async () => {});
    expect(result.current.available).toBe(true);
    let finish!: (value: ReturnType<typeof discordFixture>[]) => void;
    vi.mocked(daemon.discordConnectors).mockReturnValueOnce(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(DISCORD_STATUS_POLL_MS * 3);
    });
    expect(daemon.discordConnectors).toHaveBeenCalledTimes(2);
    await act(async () => {
      finish([{ ...c, status: 'degraded', unknownOutbound: 1 }]);
    });
    expect(result.current.connectors[0].unknownOutbound).toBe(1);
    unmount();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(DISCORD_STATUS_POLL_MS);
    });
    expect(daemon.discordConnectors).toHaveBeenCalledTimes(2);
  });

  it('conservatively revokes ready availability when a background status read fails', async () => {
    vi.useFakeTimers();
    const c = discordFixture({ enabled: true, status: 'ready' });
    vi.mocked(daemon.discordConnectors).mockResolvedValue([c]);
    const { result, unmount } = renderHook(() =>
      useDiscordIntegrations('agent-a', true),
    );
    await act(async () => {});
    expect(result.current.available).toBe(true);
    vi.mocked(daemon.discordConnectors).mockRejectedValue(
      new Error('read failed'),
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(DISCORD_STATUS_POLL_MS);
    });
    expect(result.current.available).toBe(false);
    expect(result.current.connectors).toEqual([c]);
    unmount();
  });

  it('refreshes partial mutation failures, retains their safe errors and does not poll during a mutation', async () => {
    vi.useFakeTimers();
    const c = discordFixture({ enabled: true, status: 'ready' });
    vi.mocked(daemon.discordConnectors).mockResolvedValue([c]);
    const { result, unmount } = renderHook(() =>
      useDiscordIntegrations('agent-a', true),
    );
    await act(async () => {});
    let reject!: (reason: unknown) => void;
    vi.mocked(daemon.replaceDiscordCredential).mockReturnValueOnce(
      new Promise((_, fail) => {
        reject = fail;
      }),
    );
    act(() => {
      void result.current.replace(c.id, 'secret');
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(DISCORD_STATUS_POLL_MS * 2);
    });
    expect(daemon.discordConnectors).toHaveBeenCalledTimes(1);
    vi.mocked(daemon.discordConnectors).mockResolvedValue([
      { ...c, status: 'credentialRequired', enabled: false },
    ]);
    await act(async () => {
      reject({ status: 409, message: 'secret' });
    });
    expect(result.current.connectors[0].status).toBe('credentialRequired');
    expect(result.current.error).toContain('same bot');
    vi.mocked(daemon.discordConnectors).mockResolvedValue([]);
    vi.mocked(daemon.deleteDiscordConnector).mockRejectedValueOnce(
      new Error('partial deletion'),
    );
    await act(async () => {
      await result.current.disconnect(c.id);
    });
    expect(result.current.connectors).toEqual([]);
    expect(result.current.error).toBe(
      'Discord request failed. Refresh status and check the connection setup.',
    );
    unmount();
  });

  it('rejects old action and refresh callbacks captured before an agent switch', async () => {
    const { result, rerender } = renderHook(
      ({ id }) => useDiscordIntegrations(id),
      { initialProps: { id: 'agent-a' } },
    );
    await waitFor(() => expect(result.current.loading).toBe(false));
    const old = result.current;
    rerender({ id: 'agent-b' });
    await waitFor(() => expect(result.current.loading).toBe(false));
    vi.mocked(daemon.discordConnectors).mockClear();
    await act(async () => {
      expect(
        await old.create({
          botToken: 'secret',
          guildId: '3',
          channelId: '2',
          approvedUserIds: ['4'],
          privateChannelConfirmed: true,
        }),
      ).toBe(false);
      await old.refresh();
    });
    expect(daemon.createDiscordConnector).not.toHaveBeenCalled();
    expect(daemon.discordConnectors).not.toHaveBeenCalled();
  });
});

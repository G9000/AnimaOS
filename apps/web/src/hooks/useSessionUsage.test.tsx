import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { daemon } from '../lib/daemon-api';
import { sessionFixture } from '../test/sessions';
import { totalsFixture } from '../test/usage';
import { useSessionDetails, useSessionUsage } from './useSessionUsage';

const target = { agentId: 'agent-main', sessionId: 'chat:1' };

afterEach(() => {
  vi.restoreAllMocks();
});

describe('useSessionUsage', () => {
  it('reads the context estimate with usage and refreshes both through the same session request', async () => {
    const contextUsage = {
      estimatedTokens: 100,
      budgetTokens: 1000,
      replyReserveTokens: 200,
    };
    const get = vi.spyOn(daemon, 'getSession').mockResolvedValue(
      sessionFixture('chat:1', {
        usage: totalsFixture({ calls: 2 }),
        contextUsage,
      }),
    );
    const { result, rerender } = renderHook(
      ({ key }) => useSessionDetails(target, key),
      {
        initialProps: { key: 'messages:1' },
      },
    );
    await waitFor(() =>
      expect(result.current.contextUsage).toEqual(contextUsage),
    );
    expect(result.current.usage?.calls).toBe(2);
    expect(get).toHaveBeenCalledTimes(1);
    get.mockResolvedValue(
      sessionFixture('chat:1', {
        contextUsage: { ...contextUsage, estimatedTokens: 200 },
      }),
    );
    rerender({ key: 'messages:2' });
    await waitFor(() =>
      expect(result.current.contextUsage?.estimatedTokens).toBe(200),
    );
    expect(get).toHaveBeenCalledTimes(2);
  });

  it('does not expose context estimates from a failed read or an offline session', async () => {
    const get = vi
      .spyOn(daemon, 'getSession')
      .mockResolvedValueOnce(
        sessionFixture('chat:1', {
          usage: totalsFixture({ calls: 2 }),
          contextUsage: {
            estimatedTokens: 100,
            budgetTokens: 1000,
            replyReserveTokens: 200,
          },
        }),
      )
      .mockRejectedValueOnce(new TypeError('offline'));
    const { result, rerender } = renderHook(
      ({ key, enabled }) => useSessionDetails(target, key, enabled),
      {
        initialProps: { key: 0, enabled: true },
      },
    );
    await waitFor(() =>
      expect(result.current.contextUsage?.estimatedTokens).toBe(100),
    );
    rerender({ key: 1, enabled: true });
    await waitFor(() => expect(result.current.contextUsage).toBeUndefined());
    expect(result.current.usage?.calls).toBe(2);
    rerender({ key: 2, enabled: false });
    await act(async () => undefined);
    expect(get).toHaveBeenCalledTimes(2);
    expect(result.current.contextUsage).toBeUndefined();
  });

  it('rejects a stale estimate when switching agent or session', async () => {
    let release!: (session: ReturnType<typeof sessionFixture>) => void;
    vi.spyOn(daemon, 'getSession')
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            release = resolve;
          }),
      )
      .mockResolvedValueOnce(
        sessionFixture('other', {
          agentId: 'other-agent',
          contextUsage: {
            estimatedTokens: 200,
            budgetTokens: 1000,
            replyReserveTokens: 200,
          },
        }),
      );
    const { result, rerender } = renderHook(
      ({ currentTarget }) => useSessionDetails(currentTarget, 0),
      {
        initialProps: { currentTarget: target },
      },
    );
    rerender({ currentTarget: { agentId: 'other-agent', sessionId: 'other' } });
    await waitFor(() =>
      expect(result.current.contextUsage?.estimatedTokens).toBe(200),
    );
    await act(async () =>
      release(
        sessionFixture('chat:1', {
          contextUsage: {
            estimatedTokens: 900,
            budgetTokens: 1000,
            replyReserveTokens: 200,
          },
        }),
      ),
    );
    expect(result.current.contextUsage?.estimatedTokens).toBe(200);
  });

  it('reads the session’s totals', async () => {
    vi.spyOn(daemon, 'getSession').mockResolvedValue(
      sessionFixture('chat:1', { usage: totalsFixture({ calls: 2 }) }),
    );
    const { result } = renderHook(() => useSessionUsage(target, 0));
    await waitFor(() => expect(result.current?.calls).toBe(2));
    expect(daemon.getSession).toHaveBeenCalledWith('agent-main', 'chat:1');
  });

  it('reads again when the key changes and keeps the last value meanwhile', async () => {
    let release: (value: ReturnType<typeof sessionFixture>) => void = () =>
      undefined;
    vi.spyOn(daemon, 'getSession')
      .mockResolvedValueOnce(
        sessionFixture('chat:1', { usage: totalsFixture({ calls: 2 }) }),
      )
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            release = resolve;
          }),
      );
    const { result, rerender } = renderHook(
      ({ key }) => useSessionUsage(target, key),
      { initialProps: { key: 0 } },
    );
    await waitFor(() => expect(result.current?.calls).toBe(2));

    rerender({ key: 1 });
    expect(result.current?.calls).toBe(2);
    await act(async () => {
      release(sessionFixture('chat:1', { usage: totalsFixture({ calls: 3 }) }));
    });
    expect(result.current?.calls).toBe(3);
  });

  it('nothing is read without a target', async () => {
    vi.spyOn(daemon, 'getSession').mockResolvedValue(sessionFixture('chat:1'));
    const { result } = renderHook(() => useSessionUsage(null, 0));
    await act(async () => undefined);
    expect(daemon.getSession).not.toHaveBeenCalled();
    expect(result.current).toBeNull();
  });

  it('a failed read keeps the last value', async () => {
    vi.spyOn(daemon, 'getSession')
      .mockResolvedValueOnce(
        sessionFixture('chat:1', { usage: totalsFixture({ calls: 2 }) }),
      )
      .mockRejectedValueOnce(new TypeError('offline'));
    const { result, rerender } = renderHook(
      ({ key }) => useSessionUsage(target, key),
      { initialProps: { key: 0 } },
    );
    await waitFor(() => expect(result.current?.calls).toBe(2));
    rerender({ key: 1 });
    await waitFor(() =>
      expect(vi.mocked(daemon.getSession)).toHaveBeenCalledTimes(2),
    );
    await act(async () => undefined);
    expect(result.current?.calls).toBe(2);
  });
});

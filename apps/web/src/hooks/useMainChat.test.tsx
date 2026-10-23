import { act, renderHook, waitFor } from '@testing-library/react';
import { StrictMode, type ReactNode } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { DaemonTooOldError, type Session } from '@animaos/sdk';
import { daemon } from '../lib/daemon-api';
import { sessionFixture } from '../test/sessions';
import { useAttachmentSession } from './useAttachmentSession';
import { useMainChat } from './useMainChat';

function main(id = 'main', agentId = 'agent-main'): Session {
  return sessionFixture(id, { agentId, mainChat: true });
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

afterEach(() => vi.restoreAllMocks());

describe('useMainChat', () => {
  it('keeps one pending resolution through StrictMode effect replay', async () => {
    const pending = deferred<Session>();
    const resolveMain = vi
      .spyOn(daemon, 'ensureMainSession')
      .mockReturnValue(pending.promise);
    const { result } = renderHook(
      () => useMainChat({ agentId: 'agent-main', online: true }),
      {
        wrapper: ({ children }: { children: ReactNode }) => (
          <StrictMode>{children}</StrictMode>
        ),
      },
    );
    const request = result.current.ensureSession();
    expect(resolveMain).toHaveBeenCalledOnce();
    const resolved = main();
    await act(async () => pending.resolve(resolved));
    await expect(request).resolves.toBe(resolved);
    expect(result.current.session).toBe(resolved);
    expect(resolveMain).toHaveBeenCalledOnce();
  });

  it('shares initial, upload, and send resolution and survives incidental rerenders', async () => {
    const pending = deferred<Session>();
    const resolveMain = vi
      .spyOn(daemon, 'ensureMainSession')
      .mockReturnValue(pending.promise);
    const create = vi.spyOn(daemon, 'createSession');
    const { result, rerender } = renderHook(() => {
      const chat = useMainChat({ agentId: 'agent-main', online: true });
      const attachments = useAttachmentSession({
        chatKey: 'home',
        agentId: 'agent-main',
        session: chat.session,
        createSession: () => chat.ensureSession(),
      });
      return { chat, attachments };
    });
    let send!: Promise<Session>;
    let upload!: Promise<Session>;
    act(() => {
      send = result.current.chat.ensureSession();
      upload = result.current.attachments.ensureSession();
    });
    expect(send).toBe(upload);
    expect(resolveMain).toHaveBeenCalledExactlyOnceWith('agent-main');
    expect(result.current.chat.loading).toBe(true);
    rerender();
    const resolved = main();
    await act(async () => pending.resolve(resolved));
    await expect(send).resolves.toBe(resolved);
    expect(result.current.chat.session).toBe(resolved);
    rerender();
    expect(result.current.chat.session).toBe(resolved);
    await expect(result.current.chat.ensureSession()).resolves.toBe(resolved);
    expect(resolveMain).toHaveBeenCalledOnce();
    expect(create).not.toHaveBeenCalled();
  });

  it('does not expose a previous agent and rejects its stale in-flight result', async () => {
    const old = deferred<Session>();
    const next = deferred<Session>();
    vi.spyOn(daemon, 'ensureMainSession')
      .mockReturnValueOnce(old.promise)
      .mockReturnValueOnce(next.promise);
    const { result, rerender } = renderHook(useMainChat, {
      initialProps: { agentId: 'agent-main' as string | null, online: true },
    });
    const oldRequest = result.current.ensureSession();
    const rejected = oldRequest.then(
      () => ({ status: 'fulfilled', reason: undefined }),
      (reason: unknown) => ({ status: 'rejected', reason }),
    );
    rerender({ agentId: 'agent-next', online: true });
    expect(result.current.session).toBeNull();
    const resolved = main('next', 'agent-next');
    await act(async () => next.resolve(resolved));
    await act(async () => old.resolve(main('old')));
    const outcome = await rejected;
    expect(outcome.status).toBe('rejected');
    expect(outcome.reason).toBeInstanceOf(Error);
    expect(outcome.reason).toMatchObject({
      message: expect.stringMatching(/changed|active/i),
    });
    expect(result.current.session).toBe(resolved);
    expect(result.current.error).toBeNull();
  });

  it('resolves afresh after reset even for the same agent and ignores the previous generation', async () => {
    const old = deferred<Session>();
    const fresh = main('fresh');
    const resolveMain = vi
      .spyOn(daemon, 'ensureMainSession')
      .mockReturnValueOnce(old.promise)
      .mockResolvedValueOnce(fresh);
    const { result, rerender } = renderHook(useMainChat, {
      initialProps: { agentId: 'agent-main' as string | null, online: true },
    });
    rerender({ agentId: null, online: true });
    expect(result.current.session).toBeNull();
    rerender({ agentId: 'agent-main', online: true });
    await waitFor(() => expect(result.current.session).toBe(fresh));
    await act(async () => old.resolve(main('old')));
    expect(result.current.session).toBe(fresh);
    expect(resolveMain).toHaveBeenCalledTimes(2);
  });

  it('discards a request completed after unmount', async () => {
    const pending = deferred<Session>();
    vi.spyOn(daemon, 'ensureMainSession').mockReturnValue(pending.promise);
    const { result, unmount } = renderHook(() =>
      useMainChat({ agentId: 'agent-main', online: true }),
    );
    const request = result.current.ensureSession();
    const rejected = request.then(
      () => ({ status: 'fulfilled', reason: undefined }),
      (reason: unknown) => ({ status: 'rejected', reason }),
    );
    unmount();
    pending.resolve(main());
    const outcome = await rejected;
    expect(outcome.status).toBe('rejected');
    expect(outcome.reason).toBeInstanceOf(Error);
    expect(outcome.reason).toMatchObject({
      message: expect.stringMatching(/changed|active/i),
    });
  });

  it('does not initiate offline and keeps only the current agent result across disconnection', async () => {
    const resolved = main();
    const resolveMain = vi
      .spyOn(daemon, 'ensureMainSession')
      .mockResolvedValue(resolved);
    const { result, rerender } = renderHook(useMainChat, {
      initialProps: { agentId: 'agent-main' as string | null, online: false },
    });
    await expect(result.current.ensureSession()).rejects.toThrow(
      /offline|online/i,
    );
    expect(resolveMain).not.toHaveBeenCalled();
    rerender({ agentId: 'agent-main', online: true });
    await waitFor(() => expect(result.current.session).toBe(resolved));
    rerender({ agentId: 'agent-main', online: false });
    expect(result.current.session).toBe(resolved);
    await expect(result.current.ensureSession()).resolves.toBe(resolved);
    rerender({ agentId: 'other', online: false });
    expect(result.current.session).toBeNull();
    expect(resolveMain).toHaveBeenCalledOnce();
  });

  it('stops after a failed resolution until a visible retry', async () => {
    const resolved = main();
    const resolveMain = vi
      .spyOn(daemon, 'ensureMainSession')
      .mockRejectedValueOnce(new Error('save unavailable'))
      .mockResolvedValueOnce(resolved);
    const { result, rerender } = renderHook(() =>
      useMainChat({ agentId: 'agent-main', online: true }),
    );
    await waitFor(() => expect(result.current.error).toBe('save unavailable'));
    expect(result.current.loading).toBe(false);
    rerender();
    expect(resolveMain).toHaveBeenCalledOnce();
    act(() => {
      result.current.retry();
      result.current.retry();
    });
    await waitFor(() => expect(result.current.session).toBe(resolved));
    expect(result.current.error).toBeNull();
    expect(resolveMain).toHaveBeenCalledTimes(2);
  });

  it.each([
    { agentId: 'other' },
    { kind: 'helper' as const },
    { origin: 'telegram' as const },
    { archived: true },
    { mainChat: false },
    { mainChat: undefined },
    { parentSessionId: 'parent' },
    { parentRunId: 'run' },
    { parentAgentId: 'parent' },
  ])('rejects an ineligible main session: %j', async (patch) => {
    vi.spyOn(daemon, 'ensureMainSession').mockResolvedValue(main('invalid'));
    vi.mocked(daemon.ensureMainSession).mockResolvedValue(
      sessionFixture('invalid', { mainChat: true, ...patch }),
    );
    const { result } = renderHook(() =>
      useMainChat({ agentId: 'agent-main', online: true }),
    );
    await waitFor(() =>
      expect(result.current.error).toMatch(
        /main chat.*invalid|invalid.*main chat/i,
      ),
    );
    expect(result.current.session).toBeNull();
    expect(result.current.loading).toBe(false);
  });

  it('shows an update-required error without falling back to ordinary creation', async () => {
    vi.spyOn(daemon, 'ensureMainSession').mockRejectedValue(
      new DaemonTooOldError('agent-main'),
    );
    const create = vi.spyOn(daemon, 'createSession');
    const { result } = renderHook(() =>
      useMainChat({ agentId: 'agent-main', online: true }),
    );
    await waitFor(() =>
      expect(result.current.error).toMatch(/update the daemon/i),
    );
    expect(result.current.session).toBeNull();
    expect(create).not.toHaveBeenCalled();
  });
});

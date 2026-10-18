import { act, renderHook } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { daemon } from '../lib/daemon-api';
import { sessionFixture } from '../test/sessions';
import { useAttachmentSession } from './useAttachmentSession';
afterEach(() => vi.restoreAllMocks());
it('shares an injected main-session resolver between upload and send', async () => {
  const main = sessionFixture('main', { mainChat: true });
  let resolve!: (session: typeof main) => void;
  const createSession = vi.fn(
    () =>
      new Promise<typeof main>((done) => {
        resolve = done;
      }),
  );
  const legacyCreate = vi.spyOn(daemon, 'createSession');
  const { result, rerender } = renderHook(useAttachmentSession, {
    initialProps: {
      chatKey: 'home',
      agentId: 'agent',
      session: null,
      createSession,
    },
  });
  const upload = result.current.ensureSession();
  rerender({ chatKey: 'home', agentId: 'agent', session: null, createSession });
  const send = result.current.ensureSession();
  expect(upload).toBe(send);
  expect(createSession).toHaveBeenCalledExactlyOnceWith('agent');
  await act(async () => resolve(main));
  await expect(upload).resolves.toBe(main);
  expect(legacyCreate).not.toHaveBeenCalled();
});

it('lets an injected resolver retry after a failure', async () => {
  const main = sessionFixture('main');
  const createSession = vi
    .fn()
    .mockRejectedValueOnce(new Error('offline'))
    .mockResolvedValueOnce(main);
  const { result } = renderHook(() =>
    useAttachmentSession({
      chatKey: 'home',
      agentId: 'agent',
      session: null,
      createSession,
    }),
  );
  await expect(result.current.ensureSession()).rejects.toThrow('offline');
  await expect(result.current.ensureSession()).resolves.toBe(main);
  expect(createSession).toHaveBeenCalledTimes(2);
});

it('coalesces upload and first-send creation, keeps the created session for the same home chat', async () => {
  const created = sessionFixture('created');
  let resolve!: (session: typeof created) => void;
  const create = vi.spyOn(daemon, 'createSession').mockImplementation(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  const { result } = renderHook(() =>
    useAttachmentSession({ chatKey: 'home', agentId: 'agent', session: null }),
  );
  let first!: Promise<typeof created>;
  let second!: Promise<typeof created>;
  act(() => {
    first = result.current.ensureSession();
    second = result.current.ensureSession();
  });
  expect(create).toHaveBeenCalledOnce();
  expect(first).toBe(second);
  await act(async () => resolve(created));
  await expect(first).resolves.toEqual(created);
  await expect(result.current.ensureSession()).resolves.toEqual(created);
  expect(create).toHaveBeenCalledOnce();
});
it('keeps session creation bound to its original chat and permits a fresh home after adoption', async () => {
  const first = sessionFixture('first');
  const second = sessionFixture('second');
  const create = vi
    .spyOn(daemon, 'createSession')
    .mockResolvedValueOnce(first)
    .mockResolvedValueOnce(second);
  const { result, rerender } = renderHook(useAttachmentSession, {
    initialProps: {
      chatKey: 'home',
      agentId: 'agent',
      session: null as typeof first | null,
    },
  });
  await expect(result.current.ensureSession()).resolves.toEqual(first);
  act(() => result.current.forget('home'));
  rerender({ chatKey: 'existing', agentId: 'agent', session: second });
  await expect(result.current.ensureSession()).resolves.toEqual(second);
  rerender({ chatKey: 'home', agentId: 'agent', session: null });
  await expect(result.current.ensureSession()).resolves.toEqual(second);
  expect(create).toHaveBeenCalledTimes(2);
});

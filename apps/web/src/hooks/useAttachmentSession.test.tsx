import { act, renderHook } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { daemon } from '../lib/daemon-api';
import { sessionFixture } from '../test/sessions';
import { useAttachmentSession } from './useAttachmentSession';
afterEach(() => vi.restoreAllMocks());
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

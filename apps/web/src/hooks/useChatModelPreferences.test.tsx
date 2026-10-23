import { act, renderHook, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { ChatModels, ChatModelPreferencesView, Session } from '@animaos/sdk';
import { sessionFixture } from '../test/sessions';
import { useChatModelPreferences } from './useChatModelPreferences';

function deferred<T>() {
  let resolve!: (value: T) => void; let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
function view(model: string, clearGeneration = 0): ChatModelPreferencesView {
  return { preferences: { primary: { mode: 'override', provider: 'openai', model }, reviewer: { mode: 'inherit' } }, primary: { provider: 'openai', model }, reviewer: null, reviewerUnavailableReason: null, availableModels: [], appliesTo: 'next_turn', clearGeneration };
}
function fixture() {
  const get = vi.fn<ChatModels['get']>().mockResolvedValue(view('initial'));
  const update = vi.fn<ChatModels['update']>();
  const api = { get, update } as unknown as ChatModels;
  return { api, get, update };
}
const choice = (model: string) => ({ mode: 'override' as const, provider: 'openai', model });

describe('useChatModelPreferences request ordering', () => {
  it('serializes updates and retains busy without reporting a superseded failure', async () => {
    const { api, update } = fixture();
    const first = deferred<ChatModelPreferencesView>(); const second = deferred<ChatModelPreferencesView>();
    update.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const { result } = renderHook(() => useChatModelPreferences(sessionFixture('chat:one'), api, 0));
    await waitFor(() => expect(result.current.view?.primary.model).toBe('initial'));
    let firstTask!: Promise<void>; let secondTask!: Promise<void>;
    await act(async () => { firstTask = result.current.update(choice('first')); secondTask = result.current.update(choice('second')); });
    expect(update).toHaveBeenCalledTimes(1);
    expect(result.current.busy).toBe(true);
    await act(async () => { first.reject(new Error('Superseded failure')); await firstTask; });
    expect(update).toHaveBeenCalledTimes(2);
    expect(result.current.busy).toBe(true); expect(result.current.error).toBeNull();
    await act(async () => { second.resolve(view('second')); await secondTask; });
    expect(result.current.view?.primary.model).toBe('second');
    expect(result.current.busy).toBe(false); expect(result.current.error).toBeNull();
  });

  it('retains the prior confirmed update when the latest queued update fails', async () => {
    const { api, update } = fixture();
    const first = deferred<ChatModelPreferencesView>(); const second = deferred<ChatModelPreferencesView>();
    update.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const { result } = renderHook(() => useChatModelPreferences(sessionFixture('chat:one'), api, 0));
    await waitFor(() => expect(result.current.view).not.toBeNull());
    let firstTask!: Promise<void>; let secondTask!: Promise<void>;
    await act(async () => { firstTask = result.current.update(choice('confirmed')); secondTask = result.current.update(choice('refused')); });
    await act(async () => { first.resolve(view('confirmed')); await firstTask; });
    await act(async () => { second.reject(new Error('Latest change refused')); await secondTask; });
    expect(result.current.view?.primary.model).toBe('confirmed');
    expect(result.current.error).toBe('Latest change refused'); expect(result.current.busy).toBe(false);
  });

  it('does not let a refresh during an update invalidate or overwrite the mutation result', async () => {
    const { api, get, update } = fixture();
    const mutation = deferred<ChatModelPreferencesView>(); update.mockReturnValueOnce(mutation.promise);
    const refresh = deferred<ChatModelPreferencesView>(); get.mockResolvedValueOnce(view('initial')).mockReturnValueOnce(refresh.promise);
    const { result, rerender } = renderHook(({ refreshKey }) => useChatModelPreferences(sessionFixture('chat:one'), api, refreshKey), { initialProps: { refreshKey: 0 } });
    await waitFor(() => expect(result.current.view?.primary.model).toBe('initial'));
    let task!: Promise<void>;
    await act(async () => { task = result.current.update(choice('new')); });
    rerender({ refreshKey: 1 });
    await act(async () => { mutation.resolve(view('new')); await task; refresh.resolve(view('initial')); });
    expect(result.current.view?.primary.model).toBe('new');
    expect(result.current.busy).toBe(false);
  });

  it('ignores reads from before a mutation and from before an explicit adoption', async () => {
    const { api, get, update } = fixture(); const old = deferred<ChatModelPreferencesView>();
    get.mockReturnValueOnce(old.promise); update.mockResolvedValueOnce(view('new'));
    const { result, rerender } = renderHook(({ refreshKey }) => useChatModelPreferences(sessionFixture('chat:one'), api, refreshKey), { initialProps: { refreshKey: 0 } });
    await act(async () => { await result.current.update(choice('new')); old.resolve(view('old')); });
    expect(result.current.view?.primary.model).toBe('new');
    const beforeAdopt = deferred<ChatModelPreferencesView>(); get.mockReturnValueOnce(beforeAdopt.promise);
    rerender({ refreshKey: 1 });
    act(() => result.current.adopt(view('adopted')));
    await act(async () => beforeAdopt.resolve(view('old')));
    expect(result.current.view?.primary.model).toBe('adopted');
  });

  it('fences old callbacks even when returning to the same chat and generation', async () => {
    const { api, get, update } = fixture(); const old = deferred<ChatModelPreferencesView>(); update.mockReturnValueOnce(old.promise);
    const chat = sessionFixture('chat:one');
    const { result, rerender } = renderHook(({ session }) => useChatModelPreferences(session, api, 0), { initialProps: { session: chat as Session | null } });
    await waitFor(() => expect(result.current.view).not.toBeNull());
    let task!: Promise<void>; await act(async () => { task = result.current.update(choice('stale')); });
    rerender({ session: sessionFixture('chat:other') });
    get.mockResolvedValueOnce(view('returned'));
    rerender({ session: chat });
    await waitFor(() => expect(result.current.view?.primary.model).toBe('returned'));
    await act(async () => { old.reject(new Error('Old chat failure')); await task; });
    expect(result.current.error).toBeNull(); expect(result.current.busy).toBe(false);
    expect(result.current.view?.primary.model).toBe('returned');
  });

  it('rejects results for another clear generation and hides non-chat contexts', async () => {
    const { api, get, update } = fixture(); const chat = sessionFixture('chat:one', { clearGeneration: 2 });
    get.mockResolvedValueOnce(view('wrong', 1)).mockResolvedValueOnce(view('correct', 2));
    const { result, rerender } = renderHook(({ session, refreshKey }) => useChatModelPreferences(session, api, refreshKey), { initialProps: { session: chat as Session | null, refreshKey: 0 } });
    await act(async () => undefined);
    expect(result.current.view).toBeNull();
    rerender({ session: chat, refreshKey: 1 });
    await waitFor(() => expect(result.current.view?.primary.model).toBe('correct'));
    update.mockResolvedValueOnce(view('stale', 1));
    await act(async () => { await result.current.update(choice('new')); });
    expect(result.current.view?.primary.model).toBe('correct');
    rerender({ session: null, refreshKey: 1 });
    act(() => result.current.adopt(view('not-a-chat')));
    expect(result.current.view).toBeNull(); expect(result.current.error).toBeNull(); expect(result.current.busy).toBe(false);
  });

  it('does not dispatch queued writes after the hook is unmounted', async () => {
    const { api, update } = fixture(); const old = deferred<ChatModelPreferencesView>(); update.mockReturnValueOnce(old.promise);
    const { result, unmount } = renderHook(() => useChatModelPreferences(sessionFixture('chat:one'), api, 0));
    let first!: Promise<void>; let second!: Promise<void>;
    await act(async () => { first = result.current.update(choice('first')); second = result.current.update(choice('second')); });
    unmount();
    await act(async () => { old.resolve(view('first')); await first; await second; });
    expect(update).toHaveBeenCalledTimes(1);
  });

  it('keeps an adopted result and discards older queued choices', async () => {
    const { api, update } = fixture(); const old = deferred<ChatModelPreferencesView>(); update.mockReturnValueOnce(old.promise);
    const { result } = renderHook(() => useChatModelPreferences(sessionFixture('chat:one'), api, 0));
    await waitFor(() => expect(result.current.view).not.toBeNull());
    let first!: Promise<void>; let second!: Promise<void>;
    await act(async () => { first = result.current.update(choice('old-first')); second = result.current.update(choice('old-second')); });
    act(() => result.current.adopt(view('adopted')));
    expect(result.current.busy).toBe(true);
    await act(async () => { old.reject(new Error('Superseded')); await first; await second; });
    expect(update).toHaveBeenCalledTimes(1);
    expect(result.current.view?.primary.model).toBe('adopted');
    expect(result.current.error).toBeNull(); expect(result.current.busy).toBe(false);
  });

  it('does not carry a pending write across a conversation clear', async () => {
    const { api, get, update } = fixture(); const old = deferred<ChatModelPreferencesView>(); update.mockReturnValueOnce(old.promise);
    const { result, rerender } = renderHook(({ session }) => useChatModelPreferences(session, api, 0), { initialProps: { session: sessionFixture('chat:one', { clearGeneration: 0 }) } });
    await waitFor(() => expect(result.current.view).not.toBeNull());
    let task!: Promise<void>; await act(async () => { task = result.current.update(choice('old')); });
    get.mockResolvedValueOnce(view('cleared', 1));
    rerender({ session: sessionFixture('chat:one', { clearGeneration: 1 }) });
    await waitFor(() => expect(result.current.view?.primary.model).toBe('cleared'));
    await act(async () => { old.resolve(view('old', 0)); await task; });
    expect(result.current.view?.primary.model).toBe('cleared');
    expect(result.current.error).toBeNull(); expect(result.current.busy).toBe(false);
  });
});

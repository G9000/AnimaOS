import { act, renderHook } from '@testing-library/react';
import { useLayoutEffect } from 'react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { READ_ALOUD_ERROR, useReadAloud } from './useReadAloud';

class Utterance {
  onstart: (() => void) | null = null;
  onend: (() => void) | null = null;
  onerror: (() => void) | null = null;
  constructor(public text: string) {}
}
const synthesis = { speak: vi.fn(), cancel: vi.fn() };
beforeEach(() => {
  synthesis.speak.mockReset();
  synthesis.cancel.mockReset();
  vi.stubGlobal('speechSynthesis', synthesis);
  vi.stubGlobal('SpeechSynthesisUtterance', Utterance);
});
afterEach(() => vi.unstubAllGlobals());
function utterance(index = 0): Utterance {
  return synthesis.speak.mock.calls[index][0];
}

it.each(['speechSynthesis', 'SpeechSynthesisUtterance'])(
  'is unsupported without %s',
  (api) => {
    vi.stubGlobal(api, undefined);
    const { result } = renderHook(() => useReadAloud('session'));
    expect(result.current.supported).toBe(false);
    act(() => result.current.read('assistant', 'Hello'));
    expect(synthesis.speak).not.toHaveBeenCalled();
  },
);

it('speaks literal visible text with hidden characters revealed and tracks actual callbacks', () => {
  const { result } = renderHook(() => useReadAloud('session'));
  act(() => result.current.read('assistant', 'Hello\u200B <b>friend</b>'));
  expect(utterance().text).toBe('Hello⟨U+200B⟩ <b>friend</b>');
  expect(synthesis.cancel).toHaveBeenCalledOnce();
  expect(result.current.speakingMessageId).toBeNull();
  act(() => utterance().onstart?.());
  expect(result.current.speakingMessageId).toBe('assistant');
  act(() => utterance().onend?.());
  expect(result.current.speakingMessageId).toBeNull();
  expect(utterance().onstart).toBeNull();
});

it('replaces an utterance and ignores saved stale start/end/error callbacks', () => {
  const { result } = renderHook(() => useReadAloud('session'));
  act(() => result.current.read('first', 'First reply'));
  const old = utterance();
  const lateStart = old.onstart;
  const lateEnd = old.onend;
  const lateError = old.onerror;
  act(() => old.onstart?.());
  act(() => result.current.read('second', 'Second reply'));
  expect(synthesis.cancel).toHaveBeenCalledTimes(2);
  expect(old.onend).toBeNull();
  act(() => utterance(1).onstart?.());
  act(() => {
    lateStart?.();
    lateEnd?.();
    lateError?.();
  });
  expect(result.current.speakingMessageId).toBe('second');
  expect(result.current.error).toBeNull();
});

it.each([false, true])(
  'same-message toggle stops even when started is %s',
  (started) => {
    const { result } = renderHook(() => useReadAloud('session'));
    act(() => result.current.read('reply', 'Reply'));
    if (started) act(() => utterance().onstart?.());
    act(() => result.current.read('reply', 'Reply'));
    expect(synthesis.speak).toHaveBeenCalledOnce();
    expect(synthesis.cancel).toHaveBeenCalledTimes(2);
    expect(result.current.speakingMessageId).toBeNull();
    expect(utterance().onerror).toBeNull();
  },
);

it.each(['session', 'disabled', 'unmount'] as const)(
  'cancels on %s and ignores old browser callbacks/actions',
  (boundary) => {
    const { result, rerender, unmount } = renderHook(
      ({ key, disabled }) => useReadAloud(key, disabled),
      { initialProps: { key: 'first', disabled: false } },
    );
    act(() => result.current.read('reply', 'Reply'));
    const old = utterance();
    const lateStart = old.onstart;
    const lateError = old.onerror;
    const staleRead = result.current.read;
    if (boundary === 'unmount') unmount();
    else
      rerender({
        key: boundary === 'session' ? 'second' : 'first',
        disabled: boundary === 'disabled',
      });
    expect(synthesis.cancel).toHaveBeenCalledTimes(2);
    expect(old.onstart).toBeNull();
    act(() => {
      lateStart?.();
      lateError?.();
      staleRead('late', 'Late');
    });
    expect(synthesis.speak).toHaveBeenCalledOnce();
    expect(result.current.error).toBeNull();
    expect(result.current.speakingMessageId).toBeNull();
  },
);

it('reports browser errors without exposing service details and clears the error on retry', () => {
  const { result } = renderHook(() => useReadAloud('session'));
  act(() => result.current.read('reply', 'Reply'));
  act(() => utterance().onerror?.());
  expect(result.current.error).toBe(READ_ALOUD_ERROR);
  expect(READ_ALOUD_ERROR).toBe(
    'Read aloud could not continue. Try again when you’re ready.',
  );
  expect(result.current.speakingMessageId).toBeNull();
  act(() => result.current.read('reply', 'Reply'));
  expect(result.current.error).toBeNull();
});

it('handles synchronous browser failure with the same inline error', () => {
  synthesis.speak.mockImplementationOnce(() => {
    throw new Error('private service details');
  });
  const { result } = renderHook(() => useReadAloud('session'));
  act(() => result.current.read('reply', 'Reply'));
  expect(result.current.error).toBe(READ_ALOUD_ERROR);
  expect(utterance().onerror).toBeNull();
});

it('ignores empty text and does not start while disabled', () => {
  const { result, rerender } = renderHook(
    (disabled) => useReadAloud('session', disabled),
    { initialProps: false },
  );
  act(() => result.current.read('reply', ' \n\t'));
  rerender(true);
  act(() => result.current.read('reply', 'Reply'));
  expect(synthesis.speak).not.toHaveBeenCalled();
  expect(synthesis.cancel).not.toHaveBeenCalled();
});

it('rejects an old start/error during the layout phase before boundary effect cleanup', () => {
  let lateStart: Utterance['onstart'] = null;
  let lateError: Utterance['onerror'] = null;
  const states: Array<ReturnType<typeof useReadAloud>> = [];
  const { result, rerender } = renderHook(
    (conversationKey) => {
      const controller = useReadAloud(conversationKey);
      useLayoutEffect(() => {
        if (conversationKey === 'second') {
          lateStart?.();
          lateError?.();
        }
      }, [conversationKey]);
      if (conversationKey === 'second') states.push(controller);
      return controller;
    },
    { initialProps: 'first' },
  );
  act(() => result.current.read('reply', 'Reply'));
  lateStart = utterance().onstart;
  lateError = utterance().onerror;
  rerender('second');
  expect(
    states.every(
      (state) => state.speakingMessageId === null && state.error === null,
    ),
  ).toBe(true);
});

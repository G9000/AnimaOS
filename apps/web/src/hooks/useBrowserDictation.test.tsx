import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { useBrowserDictation } from './useBrowserDictation';

class Recognition {
  static instances: Recognition[] = [];
  continuous = false;
  interimResults = false;
  lang = '';
  onstart: (() => void) | null = null;
  onend: (() => void) | null = null;
  onerror: ((event: { error: string }) => void) | null = null;
  onresult:
    | ((event: {
        resultIndex: number;
        results: Array<{ isFinal: boolean; 0: { transcript: string } }>;
      }) => void)
    | null = null;
  start = vi.fn();
  stop = vi.fn();
  abort = vi.fn();
  constructor() {
    Recognition.instances.push(this);
  }
}
const finalResult = {
  resultIndex: 0,
  results: [{ isFinal: true, 0: { transcript: 'Late words' } }],
};

beforeEach(() => {
  Recognition.instances = [];
  vi.stubGlobal('SpeechRecognition', Recognition);
});
afterEach(() => vi.unstubAllGlobals());

it.each(['session', 'disabled'] as const)(
  'cancels on %s boundary and ignores saved old callbacks',
  (boundary) => {
    const transcript = vi.fn();
    const { result, rerender } = renderHook(
      (options) => useBrowserDictation(transcript, options),
      { initialProps: { conversationKey: 'first', disabled: false } },
    );
    act(() => result.current.start());
    const old = Recognition.instances[0];
    const lateResult = old.onresult;
    const lateStart = old.onstart;
    const lateError = old.onerror;
    const lateEnd = old.onend;
    act(() =>
      old.onresult?.({
        resultIndex: 0,
        results: [{ isFinal: false, 0: { transcript: 'Preview' } }],
      }),
    );
    rerender({
      conversationKey: boundary === 'session' ? 'second' : 'first',
      disabled: boundary === 'disabled',
    });
    expect(old.abort).toHaveBeenCalledOnce();
    expect(old.onresult).toBeNull();
    expect(result.current.state).toBe('idle');
    expect(result.current.interim).toBe('');
    if (boundary === 'session') act(() => result.current.start());
    act(() => {
      lateResult?.(finalResult);
      lateStart?.();
      lateError?.({ error: 'not-allowed' });
      lateEnd?.();
    });
    expect(transcript).not.toHaveBeenCalled();
    expect(result.current.error).toBeNull();
    expect(result.current.state).toBe(
      boundary === 'session' ? 'starting' : 'idle',
    );
  },
);

it('does not request microphone while disabled, including a saved start callback', () => {
  const { result, rerender } = renderHook(
    (disabled) => useBrowserDictation(vi.fn(), { disabled }),
    { initialProps: false },
  );
  const start = result.current.start;
  rerender(true);
  act(() => start());
  act(() => result.current.start());
  expect(Recognition.instances).toHaveLength(0);
});

it('explicit cancel clears preview and detaches without committing a final result', () => {
  const transcript = vi.fn();
  const { result, unmount } = renderHook(() => useBrowserDictation(transcript));
  act(() => result.current.start());
  const recognition = Recognition.instances[0];
  const lateResult = recognition.onresult;
  act(() => {
    recognition.onstart?.();
    recognition.onresult?.({
      resultIndex: 0,
      results: [{ isFinal: false, 0: { transcript: 'Preview' } }],
    });
  });
  act(() => result.current.cancel());
  expect(result.current.interim).toBe('');
  expect(result.current.state).toBe('idle');
  act(() => lateResult?.(finalResult));
  expect(transcript).not.toHaveBeenCalled();
  unmount();
  act(() => result.current.start());
  expect(Recognition.instances).toHaveLength(1);
  expect(recognition.abort).toHaveBeenCalledOnce();
});
import { useLayoutEffect } from 'react';

it('rejects an old result during the layout phase before boundary effect cleanup', () => {
  const transcript = vi.fn();
  let lateResult: Recognition['onresult'] = null;
  const { result, rerender } = renderHook(
    (conversationKey) => {
      const dictation = useBrowserDictation(transcript, { conversationKey });
      useLayoutEffect(() => {
        if (conversationKey === 'second') lateResult?.(finalResult);
      }, [conversationKey]);
      return dictation;
    },
    { initialProps: 'first' },
  );
  act(() => result.current.start());
  lateResult = Recognition.instances[0].onresult;
  rerender('second');
  expect(transcript).not.toHaveBeenCalled();
});

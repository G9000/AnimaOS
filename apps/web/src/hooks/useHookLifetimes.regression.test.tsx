import { renderHook } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { useLatest } from './useLatest';
import { useRenderScope } from './useRenderScope';

describe('hook lifetime helpers', () => {
  it('keeps a scope through equivalent renders and creates a new lifetime on return', () => {
    const { result, rerender } = renderHook(
      ({ owner, enabled }) => useRenderScope(owner, enabled),
      { initialProps: { owner: 'a', enabled: true } },
    );
    const first = result.current;
    rerender({ owner: 'a', enabled: true });
    expect(result.current).toBe(first);
    rerender({ owner: 'b', enabled: true });
    expect(result.current.generation).toBe(first.generation + 1);
    rerender({ owner: 'a', enabled: true });
    expect(result.current).not.toBe(first);
    expect(result.current.generation).toBe(first.generation + 2);
    rerender({ owner: 'a', enabled: false });
    expect(result.current.generation).toBe(first.generation + 3);
  });

  it('keeps the same event ref while publishing each committed value', () => {
    const { result, rerender } = renderHook(
      (value: string) => useLatest(value),
      {
        initialProps: 'first',
      },
    );
    const reference = result.current;
    expect(reference.current).toBe('first');
    rerender('second');
    expect(result.current).toBe(reference);
    expect(reference.current).toBe('second');
  });
});

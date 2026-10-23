import { useState } from 'react';

/**
 * Adjust this component's local state before children commit, on mount or when
 * the inputs change. The callback must only call this component's state setters;
 * resource cleanup and other effects belong in useEffect/useLayoutEffect.
 */
export function useResetOnChange(
  values: readonly unknown[],
  reset: () => void,
): void {
  const [previous, setPrevious] = useState<readonly unknown[] | null>(null);
  if (
    previous !== null &&
    previous.length === values.length &&
    previous.every((value, index) => Object.is(value, values[index]))
  )
    return;
  setPrevious(values);
  reset();
}

import { useLayoutEffect, useRef } from 'react';

/** Event handlers and async completions read the latest committed value. */
export function useLatest<T>(value: T) {
  const latest = useRef(value);
  useLayoutEffect(() => {
    latest.current = value;
  });
  return latest;
}

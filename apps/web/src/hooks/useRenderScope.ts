import { useState } from 'react';

/** A render-visible identity for state that belongs to a particular selection. */
export function useRenderScope(...values: readonly unknown[]) {
  const [previous, setPrevious] = useState({ values, generation: 0 });
  if (
    previous.values.length === values.length &&
    previous.values.every((value, index) => Object.is(value, values[index]))
  )
    return previous;
  const next = { values, generation: previous.generation + 1 };
  setPrevious(next);
  return next;
}

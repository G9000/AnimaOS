import { afterEach, expect, it } from 'vitest';
import { savedRequest } from './pending';

afterEach(() => localStorage.clear());

it('ignores a malformed cached dialogue without changing the exact retry input', () => {
  const input = { requestId: 'retry-me', message: 'Hello', npcId: 'mira' };
  localStorage.setItem(
    'village.pending.save',
    JSON.stringify({
      id: input.requestId,
      kind: 'turn',
      npcId: 'mira',
      input,
      dialogue: { text: 'Broken cached dialogue' },
    }),
  );
  const pending = savedRequest('save');
  expect(pending?.input).toEqual(input);
  expect(pending?.dialogue).toBeUndefined();
});

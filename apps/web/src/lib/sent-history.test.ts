import { afterEach, describe, expect, it } from 'vitest';

import {
  loadSent,
  pushSent,
  recallNewer,
  recallOlder,
  resetSentCache,
} from './sent-history';

afterEach(() => {
  sessionStorage.clear();
  resetSentCache();
});

describe('sent history', () => {
  it('keeps sent messages per conversation across a reload', () => {
    pushSent('a', 'first');
    pushSent('a', '  second  ');
    pushSent('b', 'other');
    resetSentCache();
    expect(loadSent('a')).toEqual(['first', 'second']);
    expect(loadSent('b')).toEqual(['other']);
  });

  it('skips blanks and an immediate repeat, and caps its length', () => {
    pushSent('a', '   ');
    pushSent('a', 'again');
    pushSent('a', 'again');
    expect(loadSent('a')).toEqual(['again']);
    for (let i = 0; i < 80; i += 1) pushSent('a', `m${i}`);
    expect(loadSent('a')).toHaveLength(50);
    expect(loadSent('a').at(-1)).toBe('m79');
  });

  it('walks back through messages, then returns to the draft', () => {
    const items = ['one', 'two'];
    expect(recallOlder([], { cursor: -1, stash: '' }, '')).toBeNull();
    const first = recallOlder(items, { cursor: -1, stash: '' }, 'typing')!;
    expect(first.draft).toBe('two');
    const second = recallOlder(items, first.recall, 'two')!;
    expect(second.draft).toBe('one');
    expect(recallOlder(items, second.recall, 'one')).toBeNull();
    const back = recallNewer(items, second.recall)!;
    expect(back.draft).toBe('two');
    const end = recallNewer(items, back.recall)!;
    expect(end.draft).toBe('typing');
    expect(end.recall.cursor).toBe(-1);
    expect(recallNewer(items, end.recall)).toBeNull();
  });
});

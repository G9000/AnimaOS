import { describe, expect, it } from 'vitest';
import { approval } from '../tests/review-fixtures.js';
import {
  mergeReviewRecords,
  reviewLiteral,
  approvalInputError,
} from './session-review-state.js';

describe('session review state', () => {
  it('never restores a resolved record from an old pending response', () => {
    const resolved = approval({ status: 'allowed', revision: 2 });
    expect(mergeReviewRecords([resolved], [approval()])).toEqual([resolved]);
    expect(mergeReviewRecords([resolved], [approval({ revision: 2 })])).toEqual(
      [resolved],
    );
  });

  it('accepts newer versions and retains unrelated records when merging events', () => {
    const next = approval({ revision: 3, status: 'expired' });
    expect(mergeReviewRecords([approval()], [next])).toEqual([next]);
  });

  it('reveals terminal escape and invisible direction controls as literal text', () => {
    const text = reviewLiteral('x\u001b[2Jy\u202ez\u200b\rgo');
    expect(text).toBe('x\\u{001b}[2Jy\\u{202e}z\\u{200b}\\u{000d}go');
    expect(reviewLiteral('line\nnext')).toBe('line\nnext');
  });

  it('blocks expired requests and unsafe scoped grants but permits allow-once for truncated data', () => {
    const item = approval({ argumentsTruncated: true });
    expect(
      approvalInputError(item, { revision: 1, decision: 'allow_once' }),
    ).toBeNull();
    expect(
      approvalInputError(item, {
        revision: 1,
        decision: 'allow_session',
        matcher: item.suggestedMatcher,
      }),
    ).toContain('truncated');
    expect(
      approvalInputError(approval({ expiresAtMs: 1 }), {
        revision: 1,
        decision: 'deny',
      }),
    ).toContain('expired');
    expect(
      approvalInputError(approval(), {
        revision: 1,
        decision: 'allow_always',
        matcher: { kind: 'command_prefix', value: ' ' },
      }),
    ).toContain('empty');
    expect(
      approvalInputError(approval(), {
        revision: 1,
        decision: 'allow_always',
        matcher: { kind: 'domain', value: 'a.com' },
      }),
    ).toContain('supported');
  });
});

import React from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { approval } from '../../tests/review-fixtures.js';
import {
  cleanupInk,
  flushInk,
  pressInkKey,
  renderInk,
} from '../test-harness.js';
import { ApprovalReview } from './approval-review.js';

afterEach(cleanupInk);

describe('ApprovalReview', () => {
  it('requires a separate confirmation and includes the exact scoped matcher and revision', async () => {
    const onDecide = vi.fn().mockResolvedValue(true);
    const view = renderInk(
      <ApprovalReview
        approval={approval()}
        disabled={false}
        onDecide={onDecide}
        onBack={() => {}}
      />,
    );
    await pressInkKey(view, '\u001b[C');
    for (let i = 0; i < 4; i++) await pressInkKey(view, '\t');
    await pressInkKey(view, '\r');
    expect(onDecide).not.toHaveBeenCalled();
    expect(view.lastFrame()).toContain('Confirm approval decision');
    expect(view.lastFrame()).toContain('git status');
    await pressInkKey(view, '\r');
    expect(onDecide).toHaveBeenCalledWith({
      decision: 'allow_session',
      revision: 1,
      matcher: { kind: 'command_prefix', value: 'git status' },
    });
  });

  it('renders arguments safely and blocks truncated scope grants', async () => {
    const onDecide = vi.fn();
    const view = renderInk(
      <ApprovalReview
        approval={approval({
          arguments: '\u001b[2Jhidden\u202e',
          argumentsTruncated: true,
        })}
        disabled={false}
        onDecide={onDecide}
        onBack={() => {}}
      />,
    );
    await flushInk();
    expect(view.lastFrame()).toContain('\\u{001b}');
    expect(view.lastFrame()).toContain('\\u{202e}');
    await pressInkKey(view, '\u001b[C');
    for (let i = 0; i < 4; i++) await pressInkKey(view, '\t');
    await pressInkKey(view, '\r');
    expect(view.lastFrame()).toContain('truncated');
    expect(view.lastFrame()).not.toContain('Confirm approval decision');
    expect(onDecide).not.toHaveBeenCalled();
  });

  it('retains note on transient failure and refuses disabled confirmation', async () => {
    const onDecide = vi.fn().mockResolvedValue(false);
    const item = approval();
    const props = {
      approval: item,
      disabled: false,
      onDecide,
      onBack: () => {},
    };
    const view = renderInk(<ApprovalReview {...props} />);
    for (let i = 0; i < 3; i++) await pressInkKey(view, '\t');
    await pressInkKey(view, 'keep this note');
    await pressInkKey(view, '\t');
    await pressInkKey(view, '\r');
    view.rerender(<ApprovalReview {...props} disabled />);
    await pressInkKey(view, '\r');
    expect(onDecide).not.toHaveBeenCalled();
    view.rerender(<ApprovalReview {...props} />);
    await pressInkKey(view, '\r');
    expect(onDecide).toHaveBeenCalledWith({
      decision: 'allow_once',
      revision: 1,
      note: 'keep this note',
    });
    expect(view.lastFrame()).toContain('keep this note');
  });

  it('does not navigate when a save finishes after leaving the form', async () => {
    let finish!: (value: boolean) => void;
    const onDecide = vi.fn().mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const onBack = vi.fn();
    const view = renderInk(
      <ApprovalReview
        approval={approval()}
        disabled={false}
        onDecide={onDecide}
        onBack={onBack}
      />,
    );
    for (let i = 0; i < 4; i++) await pressInkKey(view, '\t');
    await pressInkKey(view, '\r');
    await pressInkKey(view, '\r');
    view.unmount();
    finish(true);
    await Promise.resolve();
    await Promise.resolve();
    expect(onBack).not.toHaveBeenCalled();
  });

  it.each(['allow_always', 'deny'] as const)(
    'confirms %s and forwards the optional note',
    async (decision) => {
      const onDecide = vi.fn().mockResolvedValue(true);
      const item = approval();
      const view = renderInk(
        <ApprovalReview
          approval={item}
          disabled={false}
          onDecide={onDecide}
          onBack={() => {}}
        />,
      );
      for (let i = 0; i < (decision === 'deny' ? 3 : 2); i++)
        await pressInkKey(view, '\u001b[C');
      for (let i = 0; i < 3; i++) await pressInkKey(view, '\t');
      await pressInkKey(view, 'Owner note');
      await pressInkKey(view, '\t');
      await pressInkKey(view, '\r');
      expect(onDecide).not.toHaveBeenCalled();
      await pressInkKey(view, '\r');
      expect(onDecide).toHaveBeenCalledWith({
        decision,
        revision: 1,
        note: 'Owner note',
        ...(decision === 'allow_always'
          ? { matcher: item.suggestedMatcher }
          : {}),
      });
    },
  );
});

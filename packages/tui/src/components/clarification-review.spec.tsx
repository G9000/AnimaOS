import React from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { clarification } from '../../tests/review-fixtures.js';
import { cleanupInk, pressInkKey, renderInk } from '../test-harness.js';
import { ClarificationReview } from './clarification-review.js';

afterEach(cleanupInk);

describe('ClarificationReview', () => {
  it('supports Other, multi-select with text, and a confirmed summary with explicit skipped questions', async () => {
    const onAnswer = vi.fn().mockResolvedValue(true);
    const view = renderInk(
      <ClarificationReview
        request={clarification()}
        disabled={false}
        onAnswer={onAnswer}
        onBack={() => {}}
      />,
    );
    await pressInkKey(view, ' ');
    await pressInkKey(view, '\t');
    await pressInkKey(view, 'Custom target');
    await pressInkKey(view, '\t');
    await pressInkKey(view, ' ');
    await pressInkKey(view, '\u001b[B');
    await pressInkKey(view, ' ');
    await pressInkKey(view, '\t');
    await pressInkKey(view, 'Also lint');
    await pressInkKey(view, '\t');
    await pressInkKey(view, '\t');
    await pressInkKey(view, '\r');
    expect(onAnswer).not.toHaveBeenCalled();
    expect(view.lastFrame()).toContain('Confirm clarification answers');
    expect(view.lastFrame()).toContain('Skipped');
    await pressInkKey(view, '\r');
    expect(onAnswer).toHaveBeenCalledWith({
      revision: 1,
      answers: [
        { questionId: 'q0', choiceIds: [], text: 'Custom target' },
        { questionId: 'q1', choiceIds: ['c0', 'c1'], text: 'Also lint' },
        { questionId: 'q2', choiceIds: [], text: null },
      ],
    });
  });

  it('choosing a single choice clears Other text and Escape leaves confirmation without submitting', async () => {
    const onAnswer = vi.fn();
    const onBack = vi.fn();
    const view = renderInk(
      <ClarificationReview
        request={clarification()}
        disabled={false}
        onAnswer={onAnswer}
        onBack={onBack}
      />,
    );
    await pressInkKey(view, '\t');
    await pressInkKey(view, 'discard me');
    await pressInkKey(view, '\u001b[Z');
    await pressInkKey(view, ' ');
    for (let i = 0; i < 5; i++) await pressInkKey(view, '\t');
    await pressInkKey(view, '\r');
    expect(view.lastFrame()).toContain('Local');
    expect(view.lastFrame()).not.toContain('discard me');
    await pressInkKey(view, '\u001b');
    expect(onBack).not.toHaveBeenCalled();
    expect(onAnswer).not.toHaveBeenCalled();
    await pressInkKey(view, '\u001b');
    expect(onBack).toHaveBeenCalledTimes(1);
  });

  it('does not navigate when an answer finishes after leaving the form', async () => {
    let finish!: (value: boolean) => void;
    const onAnswer = vi.fn().mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const onBack = vi.fn();
    const view = renderInk(
      <ClarificationReview
        request={clarification()}
        disabled={false}
        onAnswer={onAnswer}
        onBack={onBack}
      />,
    );
    for (let i = 0; i < 5; i++) await pressInkKey(view, '\t');
    await pressInkKey(view, '\r');
    await pressInkKey(view, '\r');
    view.unmount();
    finish(true);
    await Promise.resolve();
    await Promise.resolve();
    expect(onBack).not.toHaveBeenCalled();
  });
});

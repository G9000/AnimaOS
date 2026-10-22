import React from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { approval, clarification } from '../../tests/review-fixtures.js';
import type { SessionReview as ReviewState } from '../hooks/use-session-review.js';
import {
  cleanupInk,
  flushInk,
  pressInkKey,
  renderInk,
} from '../test-harness.js';
import { SessionReview } from './session-review.js';

afterEach(cleanupInk);

function review(overrides: Partial<ReviewState> = {}): ReviewState {
  return {
    available: true,
    approvals: [approval()],
    clarifications: [clarification()],
    canAct: true,
    loading: false,
    error: null,
    notice: null,
    busyIds: [],
    formVersions: {},
    approvalBlocked: () => false,
    clarificationBlocked: () => false,
    refresh: vi.fn().mockResolvedValue(undefined),
    onEvent: vi.fn(),
    decide: vi.fn().mockResolvedValue(false),
    answer: vi.fn().mockResolvedValue(false),
    ...overrides,
  };
}

describe('SessionReview', () => {
  it('invalidates confirmation when a new revision arrives or a conflict forces a fresh review', async () => {
    const state = review();
    const view = renderInk(
      <SessionReview review={state} filter="all" onBack={() => {}} />,
    );
    await pressInkKey(view, '\r');
    for (let i = 0; i < 4; i++) await pressInkKey(view, '\t');
    await pressInkKey(view, '\r');
    expect(view.lastFrame()).toContain('Confirm approval decision');
    view.rerender(
      <SessionReview
        review={{ ...state, formVersions: { 'a:approval-a': 1 } }}
        filter="all"
        onBack={() => {}}
      />,
    );
    await flushInk();
    expect(view.lastFrame()).not.toContain('Confirm approval decision');
    expect(view.lastFrame()).toContain('Approval request');
    for (let i = 0; i < 4; i++) await pressInkKey(view, '\t');
    await pressInkKey(view, '\r');
    expect(view.lastFrame()).toContain('Confirm approval decision');
    view.rerender(
      <SessionReview
        review={{ ...state, approvals: [approval({ revision: 2 })] }}
        filter="all"
        onBack={() => {}}
      />,
    );
    await pressInkKey(view, '\r');
    expect(view.lastFrame()).not.toContain('Confirm approval decision');
    expect(state.decide).not.toHaveBeenCalled();
  });

  it('does not substitute a different request when the open one disappears', async () => {
    const state = review();
    const view = renderInk(
      <SessionReview review={state} filter="all" onBack={() => {}} />,
    );
    await pressInkKey(view, '\r');
    view.rerender(
      <SessionReview
        review={{ ...state, approvals: [] }}
        filter="all"
        onBack={() => {}}
      />,
    );
    await flushInk();
    expect(view.lastFrame()).toContain('no longer pending');
    await pressInkKey(view, '\r');
    expect(view.lastFrame()).toContain('Which target?');
    expect(state.decide).not.toHaveBeenCalled();
  });

  it('preserves an unrelated form draft when a background request conflicts', async () => {
    const state = review({
      approvals: [approval(), approval({ id: 'approval-b', createdAtMs: 3 })],
    });
    const view = renderInk(
      <SessionReview review={state} filter="approvals" onBack={() => {}} />,
    );
    await pressInkKey(view, '\u001b[B');
    await pressInkKey(view, '\r');
    for (let i = 0; i < 3; i++) await pressInkKey(view, '\t');
    await pressInkKey(view, 'keep this draft for B');
    view.rerender(
      <SessionReview
        review={{ ...state, formVersions: { 'a:approval-a': 1 } }}
        filter="approvals"
        onBack={() => {}}
      />,
    );
    await flushInk();
    expect(view.lastFrame()).toContain('keep this draft for B');
  });
});

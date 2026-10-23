import { act, renderHook, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { useSessionInteractions } from './useSessionInteractions';
import type { ClarificationRequest } from '@animaos/sdk';

describe('useSessionInteractions', () => {
  it('clears side questions and fences late answers when the same chat resets', async () => {
    let finish!: (value: {
      answer: string;
      snapshotAtMs: number;
      snapshotThroughMessageId: null;
    }) => void;
    const client = {
      list: vi.fn().mockResolvedValue([]),
      answer: vi.fn(),
      sideQuestion: vi
        .fn()
        .mockResolvedValueOnce({
          answer: 'Old answer',
          snapshotAtMs: 1,
          snapshotThroughMessageId: null,
        })
        .mockImplementationOnce(
          () =>
            new Promise((resolve) => {
              finish = resolve;
            }),
        ),
    };
    const { result, rerender } = renderHook(
      ({ resetKey }) =>
        useSessionInteractions({
          agentId: 'a',
          sessionId: 's',
          resetKey,
          client,
        }),
      { initialProps: { resetKey: 0 } },
    );
    await act(async () => {
      await result.current.ask('Old question');
    });
    await act(async () => {
      void result.current.ask('Pending old question');
    });
    expect(result.current.sideQuestions).toHaveLength(2);
    const signal = client.sideQuestion.mock.calls[1][3].signal as AbortSignal;
    rerender({ resetKey: 1 });
    expect(signal.aborted).toBe(true);
    expect(result.current.sideQuestions).toEqual([]);
    await act(async () => {
      finish({
        answer: 'Late old answer',
        snapshotAtMs: 2,
        snapshotThroughMessageId: null,
      });
    });
    expect(result.current.sideQuestions).toEqual([]);
  });

  it('discards a clarification read from before the same chat reset', async () => {
    let finish!: (value: ClarificationRequest[]) => void;
    const client = {
      list: vi
        .fn()
        .mockImplementationOnce(
          () =>
            new Promise<ClarificationRequest[]>((resolve) => {
              finish = resolve;
            }),
        )
        .mockResolvedValue([]),
      answer: vi.fn(),
      sideQuestion: vi.fn(),
    };
    const { result, rerender } = renderHook(
      ({ resetKey }) =>
        useSessionInteractions({
          agentId: 'a',
          sessionId: 's',
          resetKey,
          client,
        }),
      { initialProps: { resetKey: 0 } },
    );
    rerender({ resetKey: 1 });
    await waitFor(() => expect(client.list).toHaveBeenCalledTimes(2));
    await act(async () => {
      finish([
        {
          id: 'old',
          agentId: 'a',
          sessionId: 's',
          runId: 'r',
          toolCallId: 'c',
          questions: [],
          revision: 1,
          createdAtMs: 1,
          status: 'cancelled',
          response: null,
        },
      ]);
    });
    expect(result.current.clarifications).toEqual([]);
  });

  it('discards a side answer from the session left behind', async () => {
    let finish!: (value: {
      answer: string;
      snapshotAtMs: number;
      snapshotThroughMessageId: null;
    }) => void;
    const client = {
      list: vi.fn().mockResolvedValue([]),
      answer: vi.fn(),
      sideQuestion: vi.fn(
        () =>
          new Promise<{
            answer: string;
            snapshotAtMs: number;
            snapshotThroughMessageId: null;
          }>((resolve) => {
            finish = resolve;
          }),
      ),
    };
    const { result, rerender } = renderHook(
      ({ sessionId }) =>
        useSessionInteractions({ agentId: 'a', sessionId, client }),
      { initialProps: { sessionId: 's1' } },
    );
    await act(async () => {
      void result.current.ask('Why?');
    });
    expect(client.sideQuestion).toHaveBeenCalledTimes(1);
    expect(result.current.sideQuestions[0]?.pending).toBe(true);
    rerender({ sessionId: 's2' });
    await act(async () =>
      finish({
        answer: 'Old answer',
        snapshotAtMs: 1,
        snapshotThroughMessageId: null,
      }),
    );
    expect(result.current.sideQuestions).toEqual([]);
  });

  it('rehydrates durable questions on opening a session', async () => {
    const request = {
      id: 'ask',
      agentId: 'a',
      sessionId: 's',
      runId: 'r',
      toolCallId: 'c',
      questions: [],
      revision: 1,
      createdAtMs: 1,
      status: 'pending' as const,
      response: null,
    };
    const client = {
      list: vi.fn().mockResolvedValue([request]),
      answer: vi.fn(),
      sideQuestion: vi.fn(),
    };
    const { result } = renderHook(() =>
      useSessionInteractions({ agentId: 'a', sessionId: 's', client }),
    );
    await waitFor(() =>
      expect(result.current.clarifications).toEqual([request]),
    );
    expect(client.list.mock.calls[0]?.slice(0, 2)).toEqual(['a', 's']);
  });

  it('does not contact the daemon for a missing scope and refuses an old request', async () => {
    const client = { list: vi.fn(), answer: vi.fn(), sideQuestion: vi.fn() };
    const { result } = renderHook(() =>
      useSessionInteractions({ agentId: 'a', sessionId: null, client }),
    );
    await act(async () => {
      await result.current.ask('Why?');
    });
    expect(client.list).not.toHaveBeenCalled();
    expect(client.sideQuestion).not.toHaveBeenCalled();
    const request = {
      id: 'ask',
      agentId: 'a',
      sessionId: 'old',
      runId: 'r',
      toolCallId: 'c',
      questions: [],
      revision: 1,
      createdAtMs: 1,
      status: 'pending' as const,
      response: null,
    };
    await expect(
      result.current.answer(request, { revision: 1, answers: [] }),
    ).rejects.toThrow('another session');
    expect(client.answer).not.toHaveBeenCalled();
  });

  it('discards a late list result from the previous session', async () => {
    let finish!: (value: ClarificationRequest[]) => void;
    const client = {
      list: vi
        .fn()
        .mockImplementationOnce(
          () =>
            new Promise<ClarificationRequest[]>((resolve) => {
              finish = resolve;
            }),
        )
        .mockResolvedValue([]),
      answer: vi.fn(),
      sideQuestion: vi.fn(),
    };
    const { result, rerender } = renderHook(
      ({ sessionId }) =>
        useSessionInteractions({ agentId: 'a', sessionId, client }),
      { initialProps: { sessionId: 'old' } },
    );
    rerender({ sessionId: 'new' });
    await waitFor(() => expect(client.list).toHaveBeenCalledTimes(2));
    const old: ClarificationRequest = {
      id: 'old',
      agentId: 'a',
      sessionId: 'old',
      runId: 'r',
      toolCallId: 'c',
      questions: [],
      revision: 1,
      createdAtMs: 1,
      status: 'pending',
      response: null,
    };
    await act(async () => finish([old]));
    expect(result.current.clarifications).toEqual([]);
    expect(result.current.error).toBeNull();
  });
});

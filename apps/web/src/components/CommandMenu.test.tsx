import { act, fireEvent, render, screen } from '@testing-library/react';
import type {
  MemorySearchResult,
  SessionMessageSearchHit,
  SessionMessageSearchPage,
} from '@animaos/sdk';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CommandMenu, type StudioCommand } from './CommandMenu';

const command: StudioCommand = {
  id: 'plans',
  title: 'Open plans',
  description: 'Open the planning page',
  group: 'Navigate',
  run: vi.fn(),
};

function hit(messageId: string, snippet: string): SessionMessageSearchHit {
  return {
    sessionId: 'chat:plans',
    title: 'Weekend plans',
    messageId,
    role: 'assistant',
    createdAtMs: 1_725_000_000_000,
    snippet,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

async function debounce() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(200);
  });
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  vi.useRealTimers();
  vi.clearAllMocks();
});

describe('CommandMenu saved message search', () => {
  it('searches memory separately and opens the selected memory result', async () => {
    const memory: MemorySearchResult = {
      id: 'memory-bank-call',
      agentId: 'agent-a',
      agentName: 'Alpha',
      type: 'fact',
      content: 'The bank call is at 2:00 PM.',
      importance: 0.7,
      createdAt: 1_725_000_000_000,
      scope: 'shared',
      score: 0.93,
    };
    const searchMessages = vi.fn(async () => ({
      results: [hit('message-bank', 'The bank call is at 2:00 PM.')],
      hasMore: false,
    }));
    const searchMemories = vi.fn(async () => [memory]);
    const onOpenMemory = vi.fn();
    const close = vi.fn();
    render(
      <CommandMenu
        commands={[]}
        close={close}
        searchMessages={searchMessages}
        searchMemories={searchMemories}
        onOpenMemory={onOpenMemory}
      />,
    );
    const input = screen.getByRole('combobox');
    fireEvent.change(input, { target: { value: 'bank' } });
    await debounce();
    expect(searchMessages).toHaveBeenCalledWith('bank', expect.any(AbortSignal));
    expect(searchMemories).toHaveBeenCalledWith('bank', expect.any(AbortSignal));
    expect(screen.getByRole('group', { name: 'Memory' })).toBeVisible();
    expect(screen.getByRole('option', { name: /Alpha · fact/ })).toBeVisible();
    fireEvent.keyDown(input, { key: 'ArrowDown' });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(close).toHaveBeenCalledOnce();
    expect(onOpenMemory).toHaveBeenCalledWith(memory);
  });

  it('debounces trimmed nonempty queries and keeps loading distinct from no matches', async () => {
    const request = deferred<SessionMessageSearchPage>();
    const searchMessages = vi.fn(
      (_query: string, _signal: AbortSignal) => request.promise,
    );
    render(
      <CommandMenu
        commands={[command]}
        close={vi.fn()}
        searchMessages={searchMessages}
        onOpenMessage={vi.fn()}
      />,
    );
    await debounce();
    expect(searchMessages).not.toHaveBeenCalled();
    fireEvent.change(screen.getByRole('combobox'), {
      target: { value: '  plans  ' },
    });
    expect(screen.getByText('Searching messages…')).toBeVisible();
    expect(screen.queryByText('No messages found.')).not.toBeInTheDocument();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(199);
    });
    expect(searchMessages).not.toHaveBeenCalled();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(searchMessages).toHaveBeenCalledWith(
      'plans',
      expect.any(AbortSignal),
    );
    await act(async () => request.resolve({ results: [], hasMore: false }));
    expect(screen.getByText('No messages found.')).toBeVisible();
    expect(screen.queryByText('Searching messages…')).not.toBeInTheDocument();
    expect(screen.getByRole('option', { name: /Open plans/ })).toBeVisible();
  });

  it('shows several matches from one conversation and selects the exact message with the keyboard', async () => {
    const first = hit('first-message', 'First plans match');
    const second = hit('second-message', 'Second plans match');
    const onOpenMessage = vi.fn();
    const close = vi.fn();
    render(
      <CommandMenu
        commands={[command]}
        close={close}
        searchMessages={vi.fn(async () => ({
          results: [first, second],
          hasMore: true,
        }))}
        onOpenMessage={onOpenMessage}
      />,
    );
    const input = screen.getByRole('combobox');
    fireEvent.change(input, { target: { value: 'plans' } });
    await debounce();
    expect(screen.getAllByRole('option')).toHaveLength(3);
    expect(screen.getAllByText('Weekend plans')).toHaveLength(2);
    expect(screen.getByText(/Narrow your search/)).toBeVisible();
    fireEvent.keyDown(input, { key: 'ArrowDown' });
    fireEvent.keyDown(input, { key: 'ArrowDown' });
    const selected = screen.getByRole('option', { selected: true });
    expect(selected).toHaveTextContent('Second plans match');
    expect(input).toHaveAttribute('aria-activedescendant', selected.id);
    fireEvent.keyDown(input, { key: 'Enter', isComposing: true });
    expect(onOpenMessage).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(close).toHaveBeenCalledOnce();
    expect(onOpenMessage).toHaveBeenCalledWith(second);
    expect(command.run).not.toHaveBeenCalled();
  });

  it('aborts superseded queries and ignores responses from an older query or selected agent', async () => {
    const first = deferred<SessionMessageSearchPage>();
    const next = deferred<SessionMessageSearchPage>();
    const otherAgent = deferred<SessionMessageSearchPage>();
    const signals: AbortSignal[] = [];
    const searchMessages = vi.fn((query: string, signal: AbortSignal) => {
      signals.push(signal);
      return query === 'first' ? first.promise : next.promise;
    });
    const props = { commands: [], close: vi.fn(), onOpenMessage: vi.fn() };
    const view = render(
      <CommandMenu {...props} searchMessages={searchMessages} />,
    );
    const input = screen.getByRole('combobox');
    fireEvent.change(input, { target: { value: 'first' } });
    await debounce();
    fireEvent.change(input, { target: { value: 'next' } });
    expect(signals[0].aborted).toBe(true);
    await debounce();
    view.rerender(
      <CommandMenu
        {...props}
        searchMessages={vi.fn(() => otherAgent.promise)}
      />,
    );
    expect(signals[1].aborted).toBe(true);
    await debounce();
    await act(async () => {
      first.resolve({
        results: [hit('stale-first', 'Stale first query')],
        hasMore: false,
      });
      next.resolve({
        results: [hit('stale-agent', 'Stale previous agent')],
        hasMore: false,
      });
      otherAgent.resolve({
        results: [hit('current', 'Current agent match')],
        hasMore: false,
      });
    });
    expect(screen.queryByText('Stale first query')).not.toBeInTheDocument();
    expect(screen.queryByText('Stale previous agent')).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole('option', { name: /Current agent match/ }),
    );
    expect(props.onOpenMessage).toHaveBeenCalledWith(
      expect.objectContaining({ messageId: 'current' }),
    );
  });

  it('shows a failed read without hiding commands and aborts when the dialog closes', async () => {
    const request = deferred<SessionMessageSearchPage>();
    let signal!: AbortSignal;
    const searchMessages = vi.fn(
      (_query: string, currentSignal: AbortSignal) => {
        signal = currentSignal;
        return request.promise;
      },
    );
    const view = render(
      <CommandMenu
        commands={[command]}
        close={vi.fn()}
        searchMessages={searchMessages}
        onOpenMessage={vi.fn()}
      />,
    );
    fireEvent.change(screen.getByRole('combobox'), {
      target: { value: 'plans' },
    });
    await debounce();
    await act(async () => request.reject(new Error('unavailable')));
    expect(
      screen.getByText('Message search is unavailable. Try again.'),
    ).toBeVisible();
    expect(screen.queryByText('No messages found.')).not.toBeInTheDocument();
    expect(screen.getByRole('option', { name: /Open plans/ })).toBeVisible();
    view.unmount();
    expect(signal.aborted).toBe(true);
  });
});

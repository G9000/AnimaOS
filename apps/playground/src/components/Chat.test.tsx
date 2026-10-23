import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { Chat } from './Chat';
import { agents, memories, type AgentSnapshot } from '../lib/api';

function agent(text: string): AgentSnapshot {
  return {
    state: {
      id: 'alpha',
      name: 'Alpha',
      status: 'idle',
      tokenUsage: { totalTokens: 0 },
    },
    messages: [{ id: 'message', role: 'assistant', content: { text } }],
  } as AgentSnapshot;
}

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  (
    globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
  container = document.createElement('div');
  document.body.appendChild(container);
  root = createRoot(container);
  vi.spyOn(memories, 'relationships').mockResolvedValue([]);
  vi.spyOn(memories, 'recall').mockResolvedValue([]);
});
afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.restoreAllMocks();
});

it('updates edited transcript content when message IDs and count are unchanged', async () => {
  await act(async () =>
    root.render(
      <Chat kind="agents" entity={agent('Before')} onAfterRun={() => {}} />,
    ),
  );
  expect(container.textContent).toContain('Before');
  await act(async () =>
    root.render(
      <Chat kind="agents" entity={agent('After')} onAfterRun={() => {}} />,
    ),
  );
  expect(container.textContent).toContain('After');
  expect(container.textContent).not.toContain('Before');
});

it('preserves an optimistic message when polling returns the same displayed transcript', async () => {
  let finish!: () => void;
  vi.spyOn(agents, 'run').mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = () =>
          resolve({
            agent: agent('Done'),
            result: { status: 'success' },
          } as Awaited<ReturnType<typeof agents.run>>);
      }),
  );
  await act(async () =>
    root.render(
      <Chat kind="agents" entity={agent('Before')} onAfterRun={() => {}} />,
    ),
  );
  const input = container.querySelector('textarea')!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      'value',
    )!.set!.call(input, 'New question');
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await act(async () =>
    input.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }),
    ),
  );
  await act(async () =>
    root.render(
      <Chat kind="agents" entity={agent('Before')} onAfterRun={() => {}} />,
    ),
  );
  expect(container.textContent).toContain('New question');
  await act(async () => finish());
  expect(container.textContent).toContain('Done');
});

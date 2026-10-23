import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import type { ChatResult } from '@animaos/sdk';
import {
  ChatResultsLayout,
  ChatResultCard,
  ChatResultsPanel,
  evaluateCalculation,
} from './ChatResultsPanel';

const report: ChatResult = {
  id: '11111111-1111-4111-8111-111111111111',
  revision: 1,
  agentId: 'a',
  sessionId: 'chat:one',
  clearGeneration: 0,
  title: 'Report <script>test</script>',
  payload: {
    kind: 'document',
    format: 'text',
    text: '<img src=x onerror=alert(1)>',
  },
  pinned: false,
  createdAtMs: 1,
  updatedAtMs: 1,
  sourceRunId: 'run',
  sourceToolCallId: 'call',
  requestKey: 'create',
};

it('renders escaped result content and persists pin only after the server acknowledges it', async () => {
  let acknowledge!: (value: { result: ChatResult }) => void;
  const api = {
    list: vi.fn().mockResolvedValue({ results: [report], clearGeneration: 0 }),
    pin: vi.fn(
      () =>
        new Promise<{ result: ChatResult }>((resolve) => {
          acknowledge = resolve;
        }),
    ),
  };
  const { container } = render(
    <ChatResultsPanel
      agentId="a"
      sessionId="chat:one"
      clearGeneration={0}
      api={api}
    />,
  );
  expect(await screen.findByText(report.title)).toBeVisible();
  expect(container.querySelector('img')).toBeNull();
  expect(screen.getByText('<img src=x onerror=alert(1)>')).toBeVisible();
  fireEvent.click(screen.getByRole('button', { name: 'Pin report' }));
  expect(api.pin).toHaveBeenCalledWith('a', 'chat:one', report.id, {
    clearGeneration: 0,
    expectedRevision: 1,
    pinned: true,
  });
  expect(screen.queryByRole('button', { name: 'Unpin report' })).toBeNull();
  await act(async () =>
    acknowledge({ result: { ...report, pinned: true, revision: 2 } }),
  );
  expect(screen.getByRole('button', { name: 'Unpin report' })).toBeVisible();
});

it('does not display delayed results or pin acknowledgements from another conversation', async () => {
  let first!: (value: {
    results: ChatResult[];
    clearGeneration: number;
  }) => void;
  const api = {
    list: vi.fn().mockImplementation((_agent, session) =>
      session === 'chat:one'
        ? new Promise((resolve) => {
            first = resolve;
          })
        : Promise.resolve({ results: [], clearGeneration: 0 }),
    ),
    pin: vi.fn(),
  };
  const view = render(
    <ChatResultsPanel
      agentId="a"
      sessionId="chat:one"
      clearGeneration={0}
      api={api}
    />,
  );
  view.rerender(
    <ChatResultsPanel
      agentId="a"
      sessionId="chat:two"
      clearGeneration={0}
      api={api}
    />,
  );
  await act(async () => first({ results: [report], clearGeneration: 0 }));
  await waitFor(() => expect(api.list).toHaveBeenCalledTimes(2));
  expect(screen.queryByText(report.title)).toBeNull();
});

it('discards a pin acknowledgement after switching conversations', async () => {
  let acknowledge!: (value: { result: ChatResult }) => void;
  const api = {
    list: vi.fn().mockImplementation((_agent, session) =>
      Promise.resolve({
        results: session === 'chat:one' ? [report] : [],
        clearGeneration: 0,
      }),
    ),
    pin: vi.fn(
      () =>
        new Promise<{ result: ChatResult }>((resolve) => {
          acknowledge = resolve;
        }),
    ),
  };
  const view = render(
    <ChatResultsPanel
      agentId="a"
      sessionId="chat:one"
      clearGeneration={0}
      api={api}
    />,
  );
  fireEvent.click(await screen.findByRole('button', { name: 'Pin report' }));
  view.rerender(
    <ChatResultsPanel
      agentId="a"
      sessionId="chat:two"
      clearGeneration={0}
      api={api}
    />,
  );
  await act(async () =>
    acknowledge({ result: { ...report, pinned: true, revision: 2 } }),
  );
  expect(screen.queryByText(report.title)).toBeNull();
  expect(screen.queryByRole('button', { name: 'Unpin report' })).toBeNull();
});

it('removes old reports when the server reports a newer reset generation', async () => {
  const api = {
    list: vi.fn().mockResolvedValue({ results: [report], clearGeneration: 0 }),
    pin: vi.fn(),
  };
  const view = render(
    <ChatResultsPanel
      agentId="a"
      sessionId="chat:one"
      clearGeneration={0}
      api={api}
    />,
  );
  expect(await screen.findByText(report.title)).toBeVisible();
  api.list.mockResolvedValue({ results: [], clearGeneration: 1 });
  view.rerender(
    <ChatResultsPanel
      agentId="a"
      sessionId="chat:one"
      clearGeneration={0}
      refreshKey={1}
      api={api}
    />,
  );
  expect(await screen.findByRole('alert')).toHaveTextContent(
    'conversation changed',
  );
  expect(screen.queryByText(report.title)).toBeNull();
});

it('lets the owner explore calculator inputs without modifying saved defaults', () => {
  const calculator: ChatResult = {
    ...report,
    payload: {
      kind: 'calculator',
      inputs: [
        { key: 'price', label: 'Price', value: 10, min: 0, max: 100 },
        { key: 'count', label: 'Count', value: 3 },
      ],
      outputLabel: 'Total',
      expression: {
        op: 'multiply',
        left: { op: 'input', key: 'price' },
        right: { op: 'input', key: 'count' },
      },
    },
  };
  render(<ChatResultCard result={calculator} />);
  expect(screen.getByText('Total: 30')).toBeVisible();
  fireEvent.change(screen.getByRole('spinbutton', { name: 'Price' }), {
    target: { value: '4' },
  });
  expect(screen.getByText('Total: 12')).toBeVisible();
  fireEvent.change(screen.getByRole('spinbutton', { name: 'Price' }), {
    target: { value: '101' },
  });
  expect(screen.getByText('Total: Enter valid values')).toBeVisible();
  expect(
    calculator.payload.kind === 'calculator' &&
      calculator.payload.inputs[0].value,
  ).toBe(10);
});

it('calculates only closed arithmetic and handles missing, nonfinite, and zero division values', () => {
  expect(
    evaluateCalculation(
      {
        op: 'multiply',
        left: { op: 'input', key: 'price' },
        right: { op: 'constant', value: 2 },
      },
      { price: 12 },
    ),
  ).toBe(24);
  expect(
    evaluateCalculation(
      {
        op: 'divide',
        left: { op: 'constant', value: 10 },
        right: { op: 'input', key: 'count' },
      },
      { count: 0 },
    ),
  ).toBeNull();
  expect(evaluateCalculation({ op: 'input', key: 'missing' }, {})).toBeNull();
  expect(
    evaluateCalculation({ op: 'input', key: 'x' }, { x: Infinity }),
  ).toBeNull();
});

it('keeps inline and pinned side reports on the same saved revision', async () => {
  const pinned = { ...report, pinned: true, revision: 2 };
  const api = {
    list: vi.fn().mockResolvedValue({ results: [pinned], clearGeneration: 0 }),
    pin: vi
      .fn()
      .mockResolvedValue({ result: { ...pinned, pinned: false, revision: 3 } }),
  };
  const view = render(
    <ChatResultsLayout
      agentId="a"
      sessionId="chat:one"
      clearGeneration={0}
      api={api}
    >
      {(inline) => <main>{inline}</main>}
    </ChatResultsLayout>,
  );
  expect(
    await screen.findByRole('complementary', { name: 'Pinned reports' }),
  ).toBeVisible();
  expect(screen.getAllByText(report.title)).toHaveLength(2);
  api.list.mockResolvedValue({
    results: [{ ...pinned, title: 'Updated saved report', revision: 3 }],
    clearGeneration: 0,
  });
  view.rerender(
    <ChatResultsLayout
      agentId="a"
      sessionId="chat:one"
      clearGeneration={0}
      refreshKey={1}
      api={api}
    >
      {(inline) => <main>{inline}</main>}
    </ChatResultsLayout>,
  );
  expect(await screen.findAllByText('Updated saved report')).toHaveLength(2);
  expect(screen.queryByText(report.title)).toBeNull();
});

import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import type { ChatResult } from '@animaos/sdk';
import { ChatResultsLayout } from './ChatResultsPanel';

const tracker: ChatResult = {
  id: '11111111-1111-4111-8111-111111111111',
  revision: 1,
  agentId: 'a',
  sessionId: 'chat:one',
  clearGeneration: 0,
  title: 'Launch tracker',
  pinned: false,
  createdAtMs: 1,
  updatedAtMs: 1,
  sourceRunId: 'run',
  sourceToolCallId: 'call',
  requestKey: 'create',
  payload: {
    kind: 'tracker',
    columns: [
      { key: 'task', label: 'Task', kind: 'text' },
      { key: 'hours', label: 'Hours', kind: 'number' },
      { key: 'done', label: 'Done', kind: 'checkbox' },
    ],
    rows: [{ id: 'first', cells: { task: 'Review', hours: 2, done: false } }],
  },
};

function layout(
  api: Parameters<typeof ChatResultsLayout>[0]['api'],
  extra = {},
) {
  return (
    <ChatResultsLayout
      agentId="a"
      sessionId="chat:one"
      clearGeneration={0}
      api={api}
      {...extra}
    >
      {(inline) => <main>{inline}</main>}
    </ChatResultsLayout>
  );
}

async function preview() {
  fireEvent.click(await screen.findByRole('button', { name: 'Open preview' }));
  return within(
    screen.getByRole('complementary', { name: 'Interactive preview' }),
  );
}

it('edits a tracker beside chat, saves exact revision and reopens durable values', async () => {
  let saved = tracker;
  const api = {
    list: vi.fn(async () => ({ results: [saved], clearGeneration: 0 })),
    pin: vi.fn(),
    save: vi.fn(async (_a, _s, _g, input) => ({
      result: (saved = { ...saved, ...input, revision: 2 }),
    })),
  };
  const view = render(layout(api));
  const panel = await preview();
  fireEvent.change(panel.getByRole('textbox', { name: 'Task, row 1' }), {
    target: { value: 'Ship' },
  });
  fireEvent.change(panel.getByRole('spinbutton', { name: 'Hours, row 1' }), {
    target: { value: '3.5' },
  });
  fireEvent.click(panel.getByRole('checkbox', { name: 'Done, row 1' }));
  fireEvent.click(panel.getByRole('button', { name: 'Save changes' }));
  await panel.findByText('Changes saved.');
  expect(api.save).toHaveBeenCalledWith(
    'a',
    'chat:one',
    0,
    expect.objectContaining({
      id: tracker.id,
      expectedRevision: 1,
      title: tracker.title,
      payload: expect.objectContaining({
        rows: [
          { id: 'first', cells: { task: 'Ship', hours: 3.5, done: true } },
        ],
      }),
    }),
  );
  view.unmount();
  render(layout(api));
  const reopened = await preview();
  expect(reopened.getByRole('textbox', { name: 'Task, row 1' })).toHaveValue(
    'Ship',
  );
  expect(reopened.getByRole('checkbox', { name: 'Done, row 1' })).toBeChecked();
});

it('keeps unsaved edits when a newer revision arrives and requires reload before saving', async () => {
  const api = {
    list: vi.fn().mockResolvedValue({ results: [tracker], clearGeneration: 0 }),
    pin: vi.fn(),
    save: vi.fn(),
  };
  const view = render(layout(api));
  const panel = await preview();
  fireEvent.change(panel.getByRole('textbox', { name: 'Task, row 1' }), {
    target: { value: 'My draft' },
  });
  api.list.mockResolvedValue({
    results: [{ ...tracker, revision: 2 }],
    clearGeneration: 0,
  });
  view.rerender(layout(api, { refreshKey: 1 }));
  await panel.findByText(/A newer version is available/);
  expect(panel.getByRole('textbox', { name: 'Task, row 1' })).toHaveValue(
    'My draft',
  );
  expect(panel.getByRole('button', { name: 'Save changes' })).toBeDisabled();
  fireEvent.click(panel.getByRole('button', { name: 'Reload saved version' }));
  expect(panel.getByRole('textbox', { name: 'Task, row 1' })).toHaveValue(
    'Review',
  );
  expect(api.save).not.toHaveBeenCalled();
});

it('keeps rejected save drafts and does not retry automatically', async () => {
  const api = {
    list: vi.fn().mockResolvedValue({ results: [tracker], clearGeneration: 0 }),
    pin: vi.fn(),
    save: vi
      .fn()
      .mockRejectedValue(
        new Error('The result changed. Refresh before saving.'),
      ),
  };
  render(layout(api));
  const panel = await preview();
  fireEvent.change(panel.getByRole('textbox', { name: 'Task, row 1' }), {
    target: { value: 'Keep me' },
  });
  fireEvent.click(panel.getByRole('button', { name: 'Save changes' }));
  await panel.findByText(/The result changed/);
  expect(panel.getByRole('textbox', { name: 'Task, row 1' })).toHaveValue(
    'Keep me',
  );
  expect(api.save).toHaveBeenCalledTimes(1);
});

it('rejects blank number edits, and supports adding and removing tracker rows', async () => {
  const api = {
    list: vi.fn().mockResolvedValue({ results: [tracker], clearGeneration: 0 }),
    pin: vi.fn(),
    save: vi.fn(),
  };
  render(layout(api));
  const panel = await preview();
  fireEvent.change(panel.getByRole('spinbutton', { name: 'Hours, row 1' }), {
    target: { value: '' },
  });
  expect(panel.getByRole('button', { name: 'Save changes' })).toBeDisabled();
  fireEvent.click(panel.getByRole('button', { name: 'Discard changes' }));
  fireEvent.click(panel.getByRole('button', { name: 'Add row' }));
  expect(panel.getByRole('textbox', { name: 'Task, row 2' })).toHaveValue('');
  fireEvent.click(panel.getByRole('button', { name: 'Remove row 2' }));
  expect(panel.queryByRole('textbox', { name: 'Task, row 2' })).toBeNull();
});

it('composes calculator and tracker widgets in one saved dashboard', async () => {
  const dashboard: ChatResult = {
    ...tracker,
    title: 'Launch dashboard',
    payload: {
      kind: 'dashboard',
      widgets: [
        {
          id: 'tracker',
          title: 'Tasks',
          payload: tracker.payload as Extract<
            ChatResult['payload'],
            { kind: 'tracker' }
          >,
        },
        {
          id: 'calc',
          title: 'Budget',
          payload: {
            kind: 'calculator',
            inputs: [{ key: 'hours', label: 'Estimated hours', value: 2 }],
            expression: {
              op: 'multiply',
              left: { op: 'input', key: 'hours' },
              right: { op: 'constant', value: 10 },
            },
            outputLabel: 'Cost',
          },
        },
      ],
    },
  };
  const api = {
    list: vi
      .fn()
      .mockResolvedValue({ results: [dashboard], clearGeneration: 0 }),
    pin: vi.fn(),
    save: vi.fn(),
  };
  render(layout(api));
  const panel = await preview();
  expect(panel.getByRole('heading', { name: 'Tasks' })).toBeVisible();
  fireEvent.change(panel.getByRole('spinbutton', { name: 'Estimated hours' }), {
    target: { value: '4' },
  });
  expect(panel.getByText('Cost: 40')).toBeVisible();
  expect(panel.getByRole('textbox', { name: 'Task, row 1' })).toHaveValue(
    'Review',
  );
});

it('discards a delayed save acknowledgment after switching conversations', async () => {
  let resolve!: (value: { result: ChatResult }) => void;
  const api = {
    list: vi.fn(async (_a, session) => ({
      results: session === 'chat:one' ? [tracker] : [],
      clearGeneration: 0,
    })),
    pin: vi.fn(),
    save: vi.fn(
      () =>
        new Promise<{ result: ChatResult }>((done) => {
          resolve = done;
        }),
    ),
  };
  const view = render(layout(api));
  const panel = await preview();
  fireEvent.change(panel.getByRole('textbox', { name: 'Task, row 1' }), {
    target: { value: 'Old session' },
  });
  fireEvent.click(panel.getByRole('button', { name: 'Save changes' }));
  view.rerender(layout(api, { sessionId: 'chat:two' }));
  await act(async () => resolve({ result: { ...tracker, revision: 2 } }));
  expect(
    screen.queryByRole('complementary', { name: 'Interactive preview' }),
  ).toBeNull();
  expect(screen.queryByText('Changes saved.')).toBeNull();
});

it('uses the newest revision when a refresh overtakes a pending save', async () => {
  let resolve!: (value: { result: ChatResult }) => void;
  const api = {
    list: vi.fn().mockResolvedValue({ results: [tracker], clearGeneration: 0 }),
    pin: vi.fn(),
    save: vi.fn(
      () =>
        new Promise<{ result: ChatResult }>((done) => {
          resolve = done;
        }),
    ),
  };
  const view = render(layout(api));
  const panel = await preview();
  fireEvent.change(panel.getByRole('textbox', { name: 'Task, row 1' }), {
    target: { value: 'Saved draft' },
  });
  fireEvent.click(panel.getByRole('button', { name: 'Save changes' }));
  const newer = {
    ...tracker,
    revision: 3,
    payload: {
      ...(tracker.payload as Extract<
        ChatResult['payload'],
        { kind: 'tracker' }
      >),
      rows: [{ id: 'first', cells: { task: 'Newest', hours: 5, done: true } }],
    },
  };
  api.list.mockResolvedValue({ results: [newer], clearGeneration: 0 });
  view.rerender(layout(api, { refreshKey: 1 }));
  await panel.findByText('Saved result · revision 3');
  await act(async () => resolve({ result: { ...tracker, revision: 2 } }));
  expect(panel.getByRole('textbox', { name: 'Task, row 1' })).toHaveValue(
    'Newest',
  );
  expect(panel.getByRole('textbox', { name: 'Task, row 1' })).toBeEnabled();
});

it('invalidates old acknowledgments across an A to B to A conversation switch', async () => {
  let resolve!: (value: { result: ChatResult }) => void;
  const api = {
    list: vi.fn(async (_a, session) => ({
      results: session === 'chat:one' ? [tracker] : [],
      clearGeneration: 0,
    })),
    pin: vi.fn(),
    save: vi.fn(
      () =>
        new Promise<{ result: ChatResult }>((done) => {
          resolve = done;
        }),
    ),
  };
  const view = render(layout(api));
  const panel = await preview();
  fireEvent.change(panel.getByRole('textbox', { name: 'Task, row 1' }), {
    target: { value: 'Old edit' },
  });
  fireEvent.click(panel.getByRole('button', { name: 'Save changes' }));
  view.rerender(layout(api, { sessionId: 'chat:two' }));
  view.rerender(layout(api));
  await screen.findByRole('button', { name: 'Open preview' });
  await act(async () =>
    resolve({
      result: { ...tracker, revision: 2, title: 'Stale acknowledgment' },
    }),
  );
  expect(screen.queryByText('Stale acknowledgment')).toBeNull();
  expect(
    screen.queryByRole('complementary', { name: 'Interactive preview' }),
  ).toBeNull();
});

it('refuses numeric cells outside the daemon safe-number range', async () => {
  const api = {
    list: vi.fn().mockResolvedValue({ results: [tracker], clearGeneration: 0 }),
    pin: vi.fn(),
    save: vi.fn(),
  };
  render(layout(api));
  const panel = await preview();
  fireEvent.change(panel.getByRole('spinbutton', { name: 'Hours, row 1' }), {
    target: { value: '1e30' },
  });
  expect(panel.getByRole('button', { name: 'Save changes' })).toBeDisabled();
});

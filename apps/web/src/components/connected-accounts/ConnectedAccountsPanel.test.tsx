import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, expect, it, vi } from 'vitest';
import type { ConnectedSource } from '@animaos/sdk';
import {
  ConnectedAccountsPanel,
  type AccountsApi,
} from './ConnectedAccountsPanel';

const source: ConnectedSource = {
  connectorId: 'mail',
  provider: 'gmail',
  kind: 'mail',
  accountLabel: 'owner@example.com',
  generation: 'g1',
  available: true,
  enabled: false,
  status: 'disabled',
  scope: '100 recent inbox messages',
  itemLimit: 100,
  itemCount: 0,
  lastAttemptAtMs: null,
  lastSuccessAtMs: null,
  error: null,
};
const template = {
  slug: 'invoice',
  revision: 2,
  title: 'Process invoice',
  parameters: [{ name: 'document' }],
  deleted: false,
};
const hit = {
  citationId: 'cite',
  connectorId: 'mail',
  provider: 'gmail',
  kind: 'mail',
  title: 'Invoice April',
  snippet: 'Amount due',
  sourceUrl: 'https://mail.google.com/mail/u/0/#inbox/abc',
  citationPath: '/api/item',
  indexedAtMs: 1,
  sourceModifiedAt: null,
  truncated: true,
};
let api: AccountsApi;
beforeEach(() => {
  api = {
    connectedSearch: {
      sources: vi.fn(async () => ({
        sources: [source],
        pollIntervalMs: 120000,
      })),
      setSource: vi.fn(async () => ({
        ...source,
        enabled: true,
        status: 'ready',
      })),
      refresh: vi.fn(async () => ({
        sources: [{ ...source, enabled: true, status: 'ready' }],
      })),
      query: vi.fn(async () => ({
        hits: [hit],
        sources: [{ ...source, enabled: true, status: 'ready' }],
      })),
      item: vi.fn(async () => ({
        ...hit,
        text: '<script>untrusted</script> Invoice total 42',
        sender: 'vendor@example.com',
        eventStart: null,
        eventEnd: null,
      })),
    },
    accountWorkflows: {
      list: vi.fn(async () => []),
      create: vi.fn(async (agentId, input) => ({
        ...input,
        id: 'rule',
        agentId,
        revision: 1,
        baselineAtMs: null,
      })),
      update: vi.fn(),
      delete: vi.fn(),
      events: vi.fn(async () => []),
    },
    workflowTemplates: { list: vi.fn(async () => [template]) },
  } as unknown as AccountsApi;
});
async function open() {
  fireEvent.click(
    screen.getByRole('button', { name: 'Search and account workflows' }),
  );
  await screen.findByText('owner@example.com');
}

it('requires source opt-in, submits its generation, and opens escaped cited content', async () => {
  render(<ConnectedAccountsPanel agentId="a" api={api} />);
  expect(api.connectedSearch.sources).not.toHaveBeenCalled();
  await open();
  expect(api.connectedSearch.setSource).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('checkbox', { name: /Use Gmail/ }));
  await screen.findByText('Account settings saved.');
  expect(api.connectedSearch.setSource).toHaveBeenCalledWith('a', 'mail', {
    enabled: true,
    expectedGeneration: 'g1',
  });
  fireEvent.change(screen.getByRole('searchbox'), {
    target: { value: 'invoice' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Search accounts' }));
  fireEvent.click(
    await screen.findByRole('button', { name: 'Read Invoice April' }),
  );
  expect(
    await screen.findByText('<script>untrusted</script> Invoice total 42'),
  ).toBeInTheDocument();
  expect(document.querySelector('script')).toBeNull();
  expect(screen.getByRole('link', { name: 'Open original' })).toHaveAttribute(
    'rel',
    'noopener noreferrer',
  );
  expect(screen.getByText(/Text was shortened/)).toBeInTheDocument();
});

it('creates a pinned workflow rule with literal filters and typed event bindings', async () => {
  vi.mocked(api.connectedSearch.sources).mockResolvedValue({
    sources: [{ ...source, enabled: true, status: 'ready' }],
    pollIntervalMs: 120000,
  });
  render(<ConnectedAccountsPanel agentId="a" api={api} />);
  await open();
  fireEvent.click(screen.getByRole('button', { name: 'New account rule' }));
  fireEvent.change(screen.getByRole('textbox', { name: 'Rule name' }), {
    target: { value: 'Invoice intake' },
  });
  fireEvent.change(screen.getByRole('textbox', { name: 'Title contains' }), {
    target: { value: 'invoice' },
  });
  fireEvent.change(
    screen.getByRole('combobox', { name: 'Value for document' }),
    { target: { value: 'text' } },
  );
  fireEvent.click(screen.getByRole('button', { name: 'Save account rule' }));
  await screen.findByText('Account rule saved.');
  expect(api.accountWorkflows.create).toHaveBeenCalledWith(
    'a',
    expect.objectContaining({
      name: 'Invoice intake',
      connectorId: 'mail',
      eventKind: 'mailObserved',
      templateSlug: 'invoice',
      templateRevision: 2,
      filters: { titleContains: 'invoice' },
      inputs: { document: { type: 'event', field: 'text' } },
      enabled: true,
    }),
  );
});

it('discards old search acknowledgments across A to B to A and offline transitions', async () => {
  let resolve!: (value: unknown) => void;
  vi.mocked(api.connectedSearch.sources).mockResolvedValue({
    sources: [{ ...source, enabled: true, status: 'ready' }],
    pollIntervalMs: 120000,
  });
  vi.mocked(api.connectedSearch.query).mockImplementation(
    () =>
      new Promise((done) => {
        resolve = done as typeof resolve;
      }),
  );
  const view = render(<ConnectedAccountsPanel agentId="a" api={api} />);
  await open();
  fireEvent.change(screen.getByRole('searchbox'), {
    target: { value: 'invoice' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Search accounts' }));
  view.rerender(<ConnectedAccountsPanel agentId="b" api={api} />);
  view.rerender(<ConnectedAccountsPanel agentId="a" api={api} />);
  await screen.findByText('owner@example.com');
  await act(async () => resolve({ hits: [hit], sources: [source] }));
  expect(screen.queryByText('Invoice April')).toBeNull();
  expect(screen.getByRole('searchbox')).toHaveValue('');
  view.rerender(
    <ConnectedAccountsPanel agentId="a" online={false} api={api} />,
  );
  expect(screen.queryByText('owner@example.com')).toBeNull();
});

it('keeps failed mutations reviewable and does not retry automatically', async () => {
  vi.mocked(api.connectedSearch.setSource).mockRejectedValue(
    new Error('Source changed. Refresh before enabling.'),
  );
  render(<ConnectedAccountsPanel agentId="a" api={api} />);
  await open();
  fireEvent.click(screen.getByRole('checkbox', { name: /Use Gmail/ }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Source changed');
  expect(api.connectedSearch.setSource).toHaveBeenCalledTimes(1);
  expect(screen.getByRole('checkbox', { name: /Use Gmail/ })).not.toBeChecked();
});

it('keeps a successful rule write when a later list read is unavailable', async () => {
  vi.mocked(api.connectedSearch.sources).mockResolvedValue({
    sources: [{ ...source, enabled: true, status: 'ready' }],
    pollIntervalMs: 120000,
  });
  render(<ConnectedAccountsPanel agentId="a" api={api} />);
  await open();
  vi.mocked(api.accountWorkflows.list).mockRejectedValue(
    new Error('List unavailable'),
  );
  fireEvent.click(screen.getByRole('button', { name: 'New account rule' }));
  fireEvent.change(screen.getByRole('textbox', { name: 'Rule name' }), {
    target: { value: 'Invoice intake' },
  });
  fireEvent.change(
    screen.getByRole('combobox', { name: 'Value for document' }),
    { target: { value: 'text' } },
  );
  fireEvent.click(screen.getByRole('button', { name: 'Save account rule' }));
  await screen.findByText('Account rule saved.');
  expect(
    screen.queryByRole('form', { name: 'Account rule editor' }),
  ).toBeNull();
  expect(
    screen.getByRole('heading', { name: 'Invoice intake' }),
  ).toBeInTheDocument();
  expect(api.accountWorkflows.create).toHaveBeenCalledTimes(1);
});

it('retains one request identity when a rule creation acknowledgment is uncertain', async () => {
  vi.mocked(api.connectedSearch.sources).mockResolvedValue({
    sources: [{ ...source, enabled: true, status: 'ready' }],
    pollIntervalMs: 120000,
  });
  vi.mocked(api.accountWorkflows.create).mockRejectedValue(
    new Error('Connection interrupted'),
  );
  render(<ConnectedAccountsPanel agentId="a" api={api} />);
  await open();
  fireEvent.click(screen.getByRole('button', { name: 'New account rule' }));
  fireEvent.change(screen.getByRole('textbox', { name: 'Rule name' }), {
    target: { value: 'Invoice intake' },
  });
  fireEvent.change(
    screen.getByRole('combobox', { name: 'Value for document' }),
    { target: { value: 'text' } },
  );
  fireEvent.click(screen.getByRole('button', { name: 'Save account rule' }));
  await screen.findByRole('alert');
  fireEvent.click(screen.getByRole('button', { name: 'Save account rule' }));
  await screen.findByRole('alert');
  const calls = vi.mocked(api.accountWorkflows.create).mock.calls;
  expect(calls).toHaveLength(2);
  expect(calls[0][1].requestKey).toBeTruthy();
  expect(calls[1][1]).toEqual(calls[0][1]);
});

it('loads account scope even if saved-workflow loading fails', async () => {
  vi.mocked(api.workflowTemplates.list).mockRejectedValue(
    new Error('Workflow list unavailable'),
  );
  render(<ConnectedAccountsPanel agentId="a" api={api} />);
  await open();
  expect(screen.getByText('100 recent inbox messages')).toBeInTheDocument();
  expect(screen.getByRole('alert')).toHaveTextContent(
    'Workflow list unavailable',
  );
});

it('clears cited text immediately when disabling a source even if the response is lost', async () => {
  vi.mocked(api.connectedSearch.sources).mockResolvedValue({
    sources: [{ ...source, enabled: true, status: 'ready' }],
    pollIntervalMs: 120000,
  });
  render(<ConnectedAccountsPanel agentId="a" api={api} />);
  await open();
  fireEvent.change(screen.getByRole('searchbox'), {
    target: { value: 'invoice' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Search accounts' }));
  fireEvent.click(
    await screen.findByRole('button', { name: 'Read Invoice April' }),
  );
  await screen.findByText('<script>untrusted</script> Invoice total 42');
  vi.mocked(api.connectedSearch.setSource).mockRejectedValue(
    new Error('Connection interrupted'),
  );
  fireEvent.click(screen.getByRole('checkbox', { name: /Use Gmail/ }));
  expect(
    screen.queryByText('<script>untrusted</script> Invoice total 42'),
  ).toBeNull();
  expect(await screen.findByRole('alert')).toHaveTextContent(
    'Connection interrupted',
  );
});

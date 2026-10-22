import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import type { DaemonCapabilities } from '@animaos/sdk';
import { daemon } from '../lib/daemon-api';
import { WorkspaceCapabilities } from './WorkspaceCapabilities';

vi.mock('../lib/daemon-api', () => ({ daemon: { capabilities: vi.fn() } }));
afterEach(() => vi.resetAllMocks());

const inventory = {
  schemaVersion: 1 as const,
  tools: [
    {
      name: 'read_file',
      description: 'Read workspace files',
      category: 'workspace' as const,
      requirements: ['Workspace folder'],
    },
  ],
  persistence: {
    controlPlane: 'durable',
    memory: 'durable',
    executionJournal: false,
  },
  extensions: [
    {
      id: 'camera',
      label: 'Camera',
      status: 'planned' as const,
      description: 'Future camera integration.',
    },
  ],
  limitations: ['Interrupted runs require review.'],
};

it('distinguishes loading, registered tools, authority and unavailable modules', async () => {
  let finish!: (value: typeof inventory) => void;
  vi.mocked(daemon.capabilities).mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  render(<WorkspaceCapabilities online />);
  expect(screen.getByRole('status')).toHaveTextContent('Loading capabilities');
  await act(async () => finish(inventory));
  expect(screen.getByText('read_file')).toBeVisible();
  expect(screen.getByText(/registration does not grant/i)).toBeVisible();
  expect(screen.getByText('Workspace folder')).toBeVisible();
  expect(screen.getByText('Planned · unavailable')).toBeVisible();
  expect(
    screen.getByText(/automatic run continuation is not available/i),
  ).toBeVisible();
  expect(
    screen.queryByRole('button', { name: /enable camera/i }),
  ).not.toBeInTheDocument();
  fireEvent.change(
    screen.getByRole('searchbox', { name: 'Search capabilities' }),
    { target: { value: 'missing tool' } },
  );
  expect(screen.getByText('No tools match your search.')).toBeVisible();
  expect(screen.queryByText('read_file')).not.toBeInTheDocument();
  expect(screen.getByText('Camera')).toBeVisible();
});

it('does not request inventory offline and allows retry after failure', async () => {
  vi.mocked(daemon.capabilities)
    .mockRejectedValueOnce(new Error('Daemon unavailable'))
    .mockResolvedValue(inventory);
  const { rerender } = render(<WorkspaceCapabilities online={false} />);
  expect(screen.getByText(/connect to the daemon/i)).toBeVisible();
  expect(daemon.capabilities).not.toHaveBeenCalled();
  rerender(<WorkspaceCapabilities online />);
  expect(await screen.findByRole('alert')).toHaveTextContent(
    'Daemon unavailable',
  );
  fireEvent.click(screen.getByRole('button', { name: 'Refresh capabilities' }));
  expect(await screen.findByText('read_file')).toBeVisible();
});

it('discards an in-flight response when the daemon disconnects', async () => {
  let finish!: (value: typeof inventory) => void;
  vi.mocked(daemon.capabilities).mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const { rerender } = render(<WorkspaceCapabilities online />);
  rerender(<WorkspaceCapabilities online={false} />);
  await act(async () => finish(inventory));
  expect(screen.queryByText('read_file')).not.toBeInTheDocument();
  expect(screen.getByText(/connect to the daemon/i)).toBeVisible();
});

it('keeps tools from newer daemon categories visible and searchable', async () => {
  vi.mocked(daemon.capabilities).mockResolvedValue({
    ...inventory,
    tools: [
      ...inventory.tools,
      {
        name: 'future_tool',
        description: 'A newer tool',
        category: 'new-category',
        requirements: ['Additional access'],
      },
    ],
  } as unknown as DaemonCapabilities);
  render(<WorkspaceCapabilities online />);
  expect(await screen.findByText('future_tool')).toBeVisible();
  expect(screen.getByRole('region', { name: 'Other tools' })).toHaveTextContent(
    'Additional access',
  );
  fireEvent.change(screen.getByRole('searchbox'), {
    target: { value: 'new-category' },
  });
  expect(screen.getByText('future_tool')).toBeVisible();
  expect(screen.queryByText('read_file')).not.toBeInTheDocument();
});

it('renders mixed implementation status and searchable browser and document groups without enable actions', async () => {
  const mixedInventory = {
    ...inventory,
    tools: [
      ...inventory.tools,
      {
        name: 'browser_observe',
        description: 'Observe a page under an owner-selected browser policy',
        category: 'browser',
        requirements: ['Owner-selected browser policy', 'Explicit tool grant'],
      },
      {
        name: 'read_document',
        description: 'Read selected PDF, DOCX, or XLSX documents',
        category: 'documents',
        requirements: ['Workspace access', 'Explicit tool grant'],
      },
    ],
    extensions: [
      {
        id: 'voice',
        label: 'Voice',
        status: 'partial',
        description:
          'Selected WAV input and selected-reply speech output; setup required.',
      },
      {
        id: 'document-engine',
        label: 'Document processing',
        status: 'partial',
        description:
          'Bounded PDF, DOCX, and XLSX extraction; creation and OCR unavailable.',
      },
      {
        id: 'community-bundles',
        label: 'Community bundles',
        status: 'partial',
        description:
          'Reviewed MCP and skill installation; no arbitrary execution bridge.',
      },
      {
        id: 'camera',
        label: 'Camera',
        status: 'planned',
        description: 'Camera observation is unavailable.',
      },
      {
        id: 'future-extension',
        label: 'Future extension',
        status: 'unknown',
        description: 'Status supplied by a newer daemon.',
      },
    ],
  } as unknown as DaemonCapabilities;
  vi.mocked(daemon.capabilities).mockResolvedValue(mixedInventory);
  render(<WorkspaceCapabilities online />);

  expect(await screen.findByText('Voice')).toBeVisible();
  expect(
    screen.getAllByText('Partial implementation · setup required'),
  ).toHaveLength(3);
  expect(screen.getByText('Planned · unavailable')).toBeVisible();
  expect(screen.getByText('Implementation status unknown')).toBeVisible();
  expect(screen.getByText('Additional capabilities')).toBeVisible();
  expect(
    screen.queryByRole('button', { name: /enable|grant/i }),
  ).not.toBeInTheDocument();

  const search = screen.getByRole('searchbox', { name: 'Search capabilities' });
  fireEvent.change(search, { target: { value: 'browser' } });
  expect(
    screen.getByRole('region', { name: 'Browser tools' }),
  ).toHaveTextContent('browser_observe');
  expect(screen.queryByText('read_document')).not.toBeInTheDocument();

  fireEvent.change(search, { target: { value: 'documents' } });
  expect(
    screen.getByRole('region', { name: 'Document tools' }),
  ).toHaveTextContent('read_document');
  expect(screen.queryByText('browser_observe')).not.toBeInTheDocument();
});

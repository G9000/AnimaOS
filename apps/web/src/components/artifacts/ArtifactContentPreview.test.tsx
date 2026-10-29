import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import type { OfficeInventoryPage } from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';
import { ArtifactContentPreview } from './ArtifactContentPreview';

const sha256 = 'a'.repeat(64);
const page: OfficeInventoryPage = { version: 1, source_scope: 'd'.repeat(64), format: 'html', source_sha256: sha256, source: { kind: 'workspace', display_name: 'report.html' }, targets: [{ id: 'html:3:8', label: 'Text 1', editable: true, value: { kind: 'html_text', start_utf8: 3, end_utf8: 8, text: 'Hello' } }], coverage: { total_targets: 1, returned_targets: 1, complete: true }, retained_hyperlink_count: 0, limitations: [], next_cursor: null };

it('refuses an inventory from bytes different from the live preview', async () => {
  vi.spyOn(daemon.documentEdits, 'inspectWorkspace').mockResolvedValue({ ...page, source_sha256: 'b'.repeat(64) });
  const select = vi.fn();
  render(<ArtifactContentPreview path="report.html" agentId="agent" sessionId="chat" source="<p>Hello</p>" sha256={sha256} onSelect={select} />);
  expect(await screen.findByRole('alert')).toHaveTextContent('file changed');
  expect(screen.queryByRole('button', { name: 'Select Text 1' })).toBeNull();
  expect(select).not.toHaveBeenCalled();
});

it('discards a late inventory after its owner scope changes and clears previous errors', async () => {
  let finish!: (value: OfficeInventoryPage) => void;
  const inspect = vi.spyOn(daemon.documentEdits, 'inspectWorkspace').mockRejectedValueOnce(new Error('Unavailable')).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; })).mockResolvedValueOnce(page);
  const { rerender } = render(<ArtifactContentPreview path="report.html" agentId="agent" sessionId="old" source="<p>Hello</p>" sha256={sha256} onSelect={vi.fn()} />);
  await screen.findByText('Unavailable');
  rerender(<ArtifactContentPreview path="report.html" agentId="agent" sessionId="pending" source="<p>Hello</p>" sha256={sha256} onSelect={vi.fn()} />);
  await waitFor(() => expect(inspect).toHaveBeenCalledTimes(2));
  expect(screen.queryByText('Unavailable')).toBeNull();
  rerender(<ArtifactContentPreview path="report.html" agentId="agent" sessionId="current" source="<p>Hello</p>" sha256={sha256} onSelect={vi.fn()} />);
  await screen.findByRole('button', { name: 'Select Text 1' });
  await act(async () => finish({ ...page, targets: [{ ...page.targets[0], label: 'Stale content' }] }));
  expect(screen.queryByRole('button', { name: 'Select Stale content' })).toBeNull();
  expect(inspect.mock.calls[1][3]?.signal?.aborted).toBe(true);
});

it.each([{ source_sha256: 'b'.repeat(64) }, { source_scope: 'b'.repeat(64) }])('invalidates an already added selection if a later page changes its source binding', async (change) => {
  const first = { ...page, coverage: { total_targets: 2, returned_targets: 1, complete: false }, next_cursor: 'next' };
  const next = { ...page, targets: [{ ...page.targets[0], id: 'second' }], coverage: { total_targets: 2, returned_targets: 1, complete: true }, ...change };
  vi.spyOn(daemon.documentEdits, 'inspectWorkspace').mockResolvedValueOnce(first).mockResolvedValueOnce(next);
  const select = vi.fn();
  render(<ArtifactContentPreview path="report.html" agentId="agent" sessionId="chat" source="<p>Hello</p>" sha256={sha256} onSelect={select} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Select Text 1' }));
  fireEvent.click(screen.getByRole('button', { name: 'Add selection to draft' }));
  expect(select).toHaveBeenLastCalledWith(expect.objectContaining({ version: 1 }));
  fireEvent.click(screen.getByRole('button', { name: 'Load more content' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('document changed');
  expect(select).toHaveBeenLastCalledWith(null);
  expect(screen.getByRole('button', { name: 'Add selection to draft' })).toBeDisabled();
});

it('does not offer editing from a legacy inventory without an owner source scope', async () => {
  vi.spyOn(daemon.documentEdits, 'inspectWorkspace').mockResolvedValue({ ...page, source_scope: undefined });
  render(<ArtifactContentPreview path="report.html" agentId="agent" sessionId="chat" source="<p>Hello</p>" sha256={sha256} onSelect={vi.fn()} />);
  expect(await screen.findByRole('alert')).toHaveTextContent('incomplete or changed');
  expect(screen.queryByRole('button', { name: 'Select Text 1' })).toBeNull();
});

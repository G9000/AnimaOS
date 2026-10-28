import { fireEvent, render, screen } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import { MicrosoftFilesCard, type MicrosoftFilesApi } from './MicrosoftFilesCard';

it('lets the owner choose files without widening the selection', async () => {
  const connector = { id: 'files', mode: 'oneDrive' as const, status: 'active', generation: 'g1', selectionRevision: 2, accountLabel: 'owner@example.test', approvedSiteUrl: null, selectedFiles: [] };
  const api: MicrosoftFilesApi = {
    status: vi.fn(async () => ({ configured: true, connectors: [connector] })),
    connect: vi.fn(),
    files: vi.fn(async () => ({ files: [{ key: 'file-1', name: 'Invoice.pdf', isFolder: false, mime: 'application/pdf' }], nextCursor: null })),
    select: vi.fn(async () => ({ ...connector, selectionRevision: 3, selectedFiles: [{ key: 'file-1', name: 'Invoice.pdf' }] })),
    disconnect: vi.fn(),
  };
  render(<MicrosoftFilesCard agentId="agent" api={api} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Choose OneDrive files' }));
  fireEvent.click(await screen.findByRole('checkbox', { name: 'Invoice.pdf' }));
  fireEvent.click(screen.getByRole('button', { name: 'Save selected files' }));
  await screen.findByText('Selected: Invoice.pdf');
  expect(api.select).toHaveBeenCalledWith('agent', 'files', { expectedGeneration: 'g1', expectedSelectionRevision: 2, fileKeys: ['file-1'] });
});

it('requires a site URL and presents only a Microsoft consent link', async () => {
  const api: MicrosoftFilesApi = {
    status: vi.fn(async () => ({ configured: true, connectors: [] })),
    connect: vi.fn(async () => ({ connectorId: 'files', consentUrl: 'https://login.microsoftonline.com/common/oauth2/v2.0/authorize?state=example', expiresAtMs: 100 })),
    files: vi.fn(), select: vi.fn(), disconnect: vi.fn(),
  };
  render(<MicrosoftFilesCard agentId="agent" api={api} />);
  fireEvent.change(await screen.findByRole('combobox', { name: 'Account type' }), { target: { value: 'sharePoint' } });
  expect(screen.getByRole('button', { name: 'Connect Microsoft files' })).toBeDisabled();
  fireEvent.change(screen.getByRole('textbox', { name: 'Approved site URL' }), { target: { value: 'https://example.sharepoint.com/sites/team' } });
  fireEvent.click(screen.getByRole('button', { name: 'Connect Microsoft files' }));
  expect(await screen.findByRole('link', { name: 'Continue to Microsoft' })).toHaveAttribute('rel', 'noopener noreferrer');
  expect(api.connect).toHaveBeenCalledWith('agent', { mode: 'sharePoint', approvedSiteUrl: 'https://example.sharepoint.com/sites/team' });
});

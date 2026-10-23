import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { ShareConversationDialog, shareConversationUrl } from './ShareConversationDialog';

const api = vi.hoisted(() => ({ list: vi.fn(), create: vi.fn(), revoke: vi.fn() }));
vi.mock('../../lib/daemon-api', () => ({ chatSharesApi: api }));
afterEach(() => { vi.clearAllMocks(); vi.unstubAllGlobals(); });

it('issues credentials for only the selected chat, keeps secrets out of links, and revokes access', async () => {
  const grant = { id: 'share-one', viewerId: 'viewer-one', viewerName: 'Colleague', active: true, createdAtMs: 1, revokedAtMs: null };
  api.list.mockResolvedValue({ grants: [] });
  api.create.mockResolvedValue({ grant, secret: 'separate-secret' });
  api.revoke.mockResolvedValue({ grants: [{ ...grant, active: false, revokedAtMs: 2 }] });
  const write = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal('navigator', { clipboard: { writeText: write } });
  render(<ShareConversationDialog agentId="agent" sessionId="chat:one" title="Plans" clearGeneration={4} onClose={() => {}} />);
  await screen.findByText('No viewers have access.');
  fireEvent.change(screen.getByLabelText('Viewer name'), { target: { value: 'Colleague' } });
  fireEvent.click(screen.getByRole('button', { name: 'Create viewer access' }));
  expect(await screen.findByLabelText('Viewer secret')).toHaveValue('separate-secret');
  expect(api.create).toHaveBeenCalledWith('agent', 'chat:one', { viewerName: 'Colleague', clearGeneration: 4 });
  expect(screen.getByLabelText('Share link')).not.toHaveValue(expect.stringContaining('separate-secret'));
  fireEvent.click(screen.getByRole('button', { name: 'Copy share link' }));
  await waitFor(() => expect(write).toHaveBeenCalledWith(expect.stringContaining('#/shared/share-one')));
  expect(write.mock.calls[0][0]).not.toContain('separate-secret');
  fireEvent.click(screen.getByRole('button', { name: 'Revoke Colleague' }));
  expect(await screen.findByText('Access ended')).toBeInTheDocument();
  expect(api.revoke).toHaveBeenCalledWith('agent', 'chat:one', 'share-one');
  expect(screen.queryByLabelText('Viewer secret')).not.toBeInTheDocument();
});

it('does not retain one-time credentials when changing the selected conversation', async () => {
  api.list.mockResolvedValue({ grants: [] });
  api.create.mockResolvedValue({ grant: { id: 'share-one', viewerId: 'viewer-one', viewerName: 'One', active: true }, secret: 'secret-once' });
  const { rerender } = render(<ShareConversationDialog agentId="agent" sessionId="chat:one" title="One" clearGeneration={0} onClose={() => {}} />);
  await screen.findByText('No viewers have access.');
  fireEvent.change(screen.getByLabelText('Viewer name'), { target: { value: 'One' } });
  fireEvent.click(screen.getByRole('button', { name: 'Create viewer access' }));
  await screen.findByLabelText('Viewer secret');
  rerender(<ShareConversationDialog agentId="agent" sessionId="chat:two" title="Two" clearGeneration={0} onClose={() => {}} />);
  expect(screen.queryByDisplayValue('secret-once')).not.toBeInTheDocument();
  await waitFor(() => expect(api.list).toHaveBeenLastCalledWith('agent', 'chat:two', expect.any(AbortSignal)));
});

it('strips owner launch query parameters from the non-secret share link', () => {
  window.history.replaceState({}, '', '/?anima-login=owner-launch-secret#/chat/one');
  expect(shareConversationUrl('share-one')).not.toContain('owner-launch-secret');
  expect(shareConversationUrl('share-one')).toContain('#/shared/share-one');
  window.history.replaceState({}, '', '/');
});

it('discards a late grant result when the same conversation was cleared', async () => {
  let finish!: (value: unknown) => void;
  api.list.mockResolvedValue({ grants: [] });
  api.create.mockReturnValue(new Promise((resolve) => { finish = resolve; }));
  const { rerender } = render(<ShareConversationDialog agentId="agent" sessionId="chat:one" title="One" clearGeneration={0} onClose={() => {}} />);
  await screen.findByText('No viewers have access.');
  fireEvent.change(screen.getByLabelText('Viewer name'), { target: { value: 'Stale viewer' } });
  fireEvent.click(screen.getByRole('button', { name: 'Create viewer access' }));
  expect(api.create).toHaveBeenCalledWith('agent', 'chat:one', { viewerName: 'Stale viewer', clearGeneration: 0 });
  rerender(<ShareConversationDialog agentId="agent" sessionId="chat:one" title="One" clearGeneration={1} onClose={() => {}} />);
  await act(async () => finish({ grant: { id: 'stale-grant', viewerId: 'stale-viewer', viewerName: 'Stale viewer', active: true }, secret: 'stale-secret' }));
  expect(screen.queryByDisplayValue('stale-secret')).not.toBeInTheDocument();
  expect(screen.getByLabelText('Viewer name')).toHaveValue('');
});

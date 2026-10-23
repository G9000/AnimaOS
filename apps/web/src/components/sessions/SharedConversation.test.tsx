import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { SharedConversation } from './SharedConversation';
import { clearOwnerApiKey, setOwnerApiKey } from '../../lib/owner-api-key';

afterEach(() => { vi.unstubAllGlobals(); clearOwnerApiKey(); });

it('waits for separate viewer credentials and never requests owner APIs', async () => {
  setOwnerApiKey('owner-secret-must-never-follow-viewer-fetch');
  const requests: { url: string; init?: RequestInit }[] = [];
  vi.stubGlobal('fetch', vi.fn(async (url, init) => {
    requests.push({ url: String(url), init });
    return Response.json(String(url).endsWith('/messages')
      ? { title: 'Plans', messages: [{ id: 'one', role: 'assistant', text: '<img src="https://private.example/">', createdAtMs: 1 }], nextBefore: null, attachmentsOmitted: true }
      : { id: 'share-one', title: 'Plans', viewerName: 'Colleague', readOnly: true, attachmentsOmitted: true });
  }));
  render(<SharedConversation shareId="share-one" />);
  expect(requests).toEqual([]);
  fireEvent.change(screen.getByLabelText('Viewer ID'), { target: { value: 'viewer-one' } });
  fireEvent.change(screen.getByLabelText('Viewer secret'), { target: { value: 'separate-secret' } });
  fireEvent.click(screen.getByRole('button', { name: 'Open conversation' }));
  expect(await screen.findByRole('heading', { name: 'Plans' })).toBeInTheDocument();
  expect(screen.queryByRole('img')).not.toBeInTheDocument();
  expect(screen.queryByRole('textbox', { name: /message/i })).not.toBeInTheDocument();
  expect(requests.every(({ url }) => url.startsWith('/api/shared/conversations/share-one'))).toBe(true);
  expect(requests.every(({ init }) => init?.credentials === 'omit')).toBe(true);
  expect(requests.every(({ init }) => !new Headers(init?.headers).has('x-api-key') && !new Headers(init?.headers).has('x-anima-console-session'))).toBe(true);
  expect(screen.getByText(/Attachments, tool output, and private activity are omitted/)).toBeInTheDocument();
});

it('hides the transcript when refresh reports revoked access and clears it on logout', async () => {
  let revoked = false;
  vi.stubGlobal('fetch', vi.fn(async (url) => revoked
    ? Response.json({}, { status: 401 })
    : Response.json(String(url).endsWith('/messages')
      ? { title: 'Plans', messages: [{ id: 'one', role: 'user', text: 'Shared text', createdAtMs: 1 }], nextBefore: null, attachmentsOmitted: true }
      : { id: 'share-one', title: 'Plans', viewerName: 'Colleague', readOnly: true, attachmentsOmitted: true })));
  render(<SharedConversation shareId="share-one" />);
  fireEvent.change(screen.getByLabelText('Viewer ID'), { target: { value: 'viewer-one' } });
  fireEvent.change(screen.getByLabelText('Viewer secret'), { target: { value: 'secret' } });
  fireEvent.click(screen.getByRole('button', { name: 'Open conversation' }));
  expect(await screen.findByText('Shared text')).toBeInTheDocument();
  revoked = true;
  fireEvent.click(screen.getByRole('button', { name: 'Refresh conversation' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Viewer access is unavailable');
  expect(screen.queryByText('Shared text')).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'Sign out' }));
  await waitFor(() => expect(screen.getByLabelText('Viewer secret')).toHaveValue(''));
});

it('requires a new login when the selected share changes', async () => {
  vi.stubGlobal('fetch', vi.fn(async (url) => Response.json(String(url).endsWith('/messages')
    ? { title: 'First', messages: [], nextBefore: null, attachmentsOmitted: true }
    : { title: 'First', viewerName: 'One', readOnly: true, attachmentsOmitted: true })));
  const { rerender } = render(<SharedConversation shareId="share-one" />);
  fireEvent.change(screen.getByLabelText('Viewer ID'), { target: { value: 'one' } });
  fireEvent.change(screen.getByLabelText('Viewer secret'), { target: { value: 'secret' } });
  fireEvent.click(screen.getByRole('button', { name: 'Open conversation' }));
  await screen.findByRole('heading', { name: 'First' });
  rerender(<SharedConversation shareId="share-two" />);
  expect(screen.queryByRole('heading', { name: 'First' })).not.toBeInTheDocument();
  expect(screen.getByLabelText('Viewer secret')).toHaveValue('');
});

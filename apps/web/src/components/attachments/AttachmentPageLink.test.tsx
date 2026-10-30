import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { daemon } from '../../lib/daemon-api';
import { ChatExportScope } from '../sessions/ChatExportDownload';
import { AttachmentPageLink, attachmentPageTarget } from './AttachmentPageLink';

const href = '/api/agents/agent/attachments/att_one/pages/12';
beforeEach(() => {
  vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:linked-page');
  vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
});
afterEach(() => vi.restoreAllMocks());

it('accepts only exact same-origin page references and validated opaque identities', () => {
  expect(attachmentPageTarget(href)).toEqual({
    agentId: 'agent',
    attachmentId: 'att_one',
    page: 12,
  });
  expect(attachmentPageTarget(`${window.location.origin}${href}`)).toEqual({
    agentId: 'agent',
    attachmentId: 'att_one',
    page: 12,
  });
  for (const invalid of [
    `https://elsewhere.example${href}`,
    `${href}?token=untrusted`,
    `${href}#preview`,
    '/api/agents/agent/attachments/%2Fprivate/pages/12',
    '/api/agents/%2E%2E/attachments/att_one/pages/12',
    '/api/agents/agent/attachments/att_one/pages/0',
    '/api/agents/agent/attachments/att_one/pages/101',
    '/api/agents/agent/attachments/att_one/pages/1.5',
    '/api/agents/agent/attachments/att_one/pages/%31',
  ])
    expect(attachmentPageTarget(invalid)).toBeNull();
});

it('opens a linked page only after a click through the SDK owner transport, even without a chat scope', async () => {
  const load = vi
    .spyOn(daemon, 'previewAttachmentPage')
    .mockResolvedValue(new Blob(['page'], { type: 'image/png' }));
  render(<AttachmentPageLink href={href}>See page 12</AttachmentPageLink>);
  expect(load).not.toHaveBeenCalled();
  const opener = screen.getByRole('button', { name: 'See page 12' });
  opener.focus();
  fireEvent.click(opener);
  await screen.findByRole('img', { name: 'Original PDF page 12' });
  expect(load).toHaveBeenCalledWith('agent', 'att_one', 12, {
    signal: expect.any(AbortSignal),
  });
  expect(screen.queryByRole('combobox')).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'Close page evidence' }));
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  expect(load.mock.calls[0][3]?.signal?.aborted).toBe(true);
  expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:linked-page');
  expect(opener).toHaveFocus();
});

it('keeps another agent reference and foreign origins inert in an active chat', () => {
  const load = vi.spyOn(daemon, 'previewAttachmentPage');
  for (const url of [href, `https://elsewhere.example${href}`]) {
    const view = render(
      <ChatExportScope.Provider value={{ agentId: 'other', sessionId: 'chat' }}>
        <AttachmentPageLink href={url}>Unavailable source</AttachmentPageLink>
      </ChatExportScope.Provider>,
    );
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
    view.unmount();
  }
  expect(load).not.toHaveBeenCalled();
});

it('closes page evidence and ignores a late response when the active chat changes', async () => {
  let resolve!: (blob: Blob) => void;
  const load = vi.spyOn(daemon, 'previewAttachmentPage').mockImplementation(
    () =>
      new Promise<Blob>((done) => {
        resolve = done;
      }),
  );
  const content = (sessionId: string) => (
    <ChatExportScope.Provider value={{ agentId: 'agent', sessionId }}>
      <AttachmentPageLink href={href}>See page 12</AttachmentPageLink>
    </ChatExportScope.Provider>
  );
  const view = render(content('first'));
  fireEvent.click(screen.getByRole('button', { name: 'See page 12' }));
  view.rerender(content('second'));
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  expect(load.mock.calls[0][3]?.signal?.aborted).toBe(true);
  await act(async () => resolve(new Blob(['late'], { type: 'image/png' })));
  expect(URL.createObjectURL).not.toHaveBeenCalled();
});

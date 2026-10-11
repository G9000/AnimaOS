import { act, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { daemon } from '../../lib/daemon-api';
import { MessageAttachments } from './MessageAttachments';
const uploaded = {
  type: 'image' as const,
  id: 'att_1',
  name: '<photo>\u200B.png',
  mime: 'image/png',
  size: 3,
  kind: 'image' as const,
  path: '../never-href',
  createdAtMs: 1,
};
beforeEach(() => {
  vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:authenticated');
  vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});
it('previews only authenticated Blob URLs, reveals names and handles failed image decode', async () => {
  const download = vi
    .spyOn(daemon, 'downloadAttachment')
    .mockResolvedValue(new Blob(['png'], { type: 'image/png' }));
  render(<MessageAttachments agentId="agent" attachments={[uploaded]} />);
  await act(async () => {
    await Promise.resolve();
  });
  expect(download).toHaveBeenCalledWith('agent', 'att_1', {
    signal: expect.any(AbortSignal),
  });
  const image = screen.getByRole('img');
  expect(image).toHaveAttribute('src', 'blob:authenticated');
  expect(image).toHaveAttribute('alt', '<photo>⟨U+200B⟩.png');
  fireEvent.error(image);
  expect(screen.queryByRole('img')).not.toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'Retry preview' })).toBeVisible();
});
it('downloads with SDK Blob and server filename, revoking its temporary URL', async () => {
  const download = vi
    .spyOn(daemon, 'downloadAttachment')
    .mockResolvedValue(new Blob(['text']));
  const click = vi
    .spyOn(HTMLAnchorElement.prototype, 'click')
    .mockImplementation(function () {
      expect(this.href).toBe('blob:authenticated');
      expect(this.download).toBe('brief.txt');
    });
  const { unmount } = render(
    <MessageAttachments
      agentId="agent"
      attachments={[
        {
          ...uploaded,
          type: 'file',
          kind: 'text',
          mime: 'text/plain',
          name: 'brief.txt',
        },
      ]}
    />,
  );
  await userEvent.click(
    screen.getByRole('button', { name: 'Download brief.txt' }),
  );
  expect(download).toHaveBeenCalledOnce();
  expect(click).toHaveBeenCalledOnce();
  unmount();
  expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:authenticated');
});
it('legacy attachments have no fake link and errors never expose private transport details', async () => {
  vi.spyOn(daemon, 'downloadAttachment').mockRejectedValue(
    new Error('token=secret /private/path'),
  );
  const { rerender } = render(
    <MessageAttachments
      agentId="agent"
      attachments={[{ type: 'file', name: 'legacy.txt' }]}
    />,
  );
  expect(screen.getByText('legacy.txt')).toBeVisible();
  expect(screen.queryByRole('button')).not.toBeInTheDocument();
  rerender(
    <MessageAttachments
      agentId="agent"
      attachments={[
        {
          ...uploaded,
          type: 'file',
          kind: 'document',
          mime: 'application/pdf',
        },
      ]}
    />,
  );
  await userEvent.click(screen.getByRole('button', { name: /Download/ }));
  expect(screen.getByRole('alert')).toHaveTextContent(
    'The file could not download. Try again.',
  );
  expect(screen.queryByText(/secret/)).not.toBeInTheDocument();
});

import { act, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { daemon } from '../../lib/daemon-api';
import {
  describeDocumentExtraction,
  MessageAttachments,
} from './MessageAttachments';
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

it('shows bounded main-body extraction evidence beside the original download', async () => {
  const download = vi
    .spyOn(daemon, 'downloadAttachment')
    .mockResolvedValue(new Blob(['docx']));
  const attachment = {
    ...uploaded,
    type: 'file' as const,
    kind: 'document' as const,
    mime: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
    name: 'selected.docx',
    extraction: {
      version: 1 as const,
      status: 'ready' as const,
      paragraphCount: 3,
      omissions: ['main_body_only' as const],
    },
  };
  render(<MessageAttachments agentId="agent" attachments={[attachment]} />);

  expect(screen.getByText(/Main body text extracted/)).toHaveTextContent(
    'Main body text extracted · 3 paragraphs · Only main body text was read',
  );
  await userEvent.click(
    screen.getByRole('button', { name: 'Download selected.docx' }),
  );
  expect(download).toHaveBeenCalledWith('agent', 'att_1', {
    signal: expect.any(AbortSignal),
  });
});

it('labels partial, limited, and other closed extraction outcomes', () => {
  expect(
    describeDocumentExtraction({
      version: 1,
      status: 'partial',
      paragraphCount: 2,
      omissions: ['text_limit', 'context_limit'],
    }),
  ).toBe(
    'Some main body text extracted · 2 paragraphs · Text limit reached · Not all text fit in this message context',
  );
  expect(
    describeDocumentExtraction({
      version: 1,
      status: 'too_large',
      paragraphCount: 0,
      omissions: [],
    }),
  ).toBe('Document too large for text extraction');
  expect(
    describeDocumentExtraction({
      version: 1,
      status: 'context_omitted',
      paragraphCount: 0,
      omissions: ['context_limit'],
    }),
  ).toBe(
    'Document text not included · Not all text fit in this message context',
  );
});

it('renders partial extraction without implying the original document is complete', () => {
  render(
    <MessageAttachments
      agentId="agent"
      attachments={[
        {
          ...uploaded,
          type: 'file',
          kind: 'document',
          name: 'limited.docx',
          extraction: {
            version: 1,
            status: 'partial',
            paragraphCount: 1,
            omissions: ['paragraph_limit'],
          },
        },
      ]}
    />,
  );

  expect(screen.getByText(/Some main body text extracted/)).toHaveTextContent(
    'Some main body text extracted · 1 paragraph · Paragraph limit reached',
  );
  expect(
    screen.getByRole('button', { name: 'Download limited.docx' }),
  ).toBeVisible();
});

it('never shows ready for missing or invalid extraction metadata', () => {
  const invalid = [
    undefined,
    { version: 2, status: 'ready', paragraphCount: 1, omissions: [] },
    { version: 1, status: 'future_status', paragraphCount: 1, omissions: [] },
    { version: 1, status: 'ready', paragraphCount: 0, omissions: [] },
    { version: 1, status: 'ready', paragraphCount: 2001, omissions: [] },
    { version: 1, status: 'unsupported', paragraphCount: 1, omissions: [] },
    { version: 1, status: 'partial', paragraphCount: 1.5, omissions: [] },
    { version: 1, status: 'ready', paragraphCount: 1, omissions: ['private'] },
    {
      version: 1,
      status: 'ready',
      paragraphCount: 1,
      omissions: ['text_limit', 'text_limit'],
    },
  ];

  for (const extraction of invalid) {
    expect(describeDocumentExtraction(extraction)).toBeNull();
  }
});

it('keeps original downloads for historical documents with invalid summaries', () => {
  render(
    <MessageAttachments
      agentId="agent"
      attachments={[
        {
          ...uploaded,
          type: 'file',
          kind: 'document',
          name: 'historical.docx',
          extraction: {
            version: 9,
            status: 'ready',
            paragraphCount: 1,
            omissions: [],
          },
        } as never,
      ]}
    />,
  );

  expect(
    screen.queryByText(/Main body text extracted/),
  ).not.toBeInTheDocument();
  expect(
    screen.getByRole('button', { name: 'Download historical.docx' }),
  ).toBeVisible();
});

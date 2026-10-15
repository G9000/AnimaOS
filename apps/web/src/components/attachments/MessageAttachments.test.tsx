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

it('describes PDF text-layer coverage with honest page counts', () => {
  expect(
    describeDocumentExtraction({
      version: 2,
      format: 'pdf',
      status: 'ready',
      pageCount: 2,
      textPageCount: 1,
      includedPageCount: 1,
      omissions: ['text_layer_only'],
    }),
  ).toBe(
    'PDF text layer extracted · 1 of 2 pages with text · 1 page included · Only the PDF text layer was read',
  );
  expect(
    describeDocumentExtraction({
      version: 2,
      format: 'pdf',
      status: 'partial',
      pageCount: 2,
      textPageCount: 2,
      includedPageCount: 1,
      omissions: ['context_limit'],
    }),
  ).toBe(
    'Some PDF text layer extracted · 2 of 2 pages with text · 1 page included · Not all PDF text fit in this message context',
  );
  expect(
    describeDocumentExtraction({
      version: 2,
      format: 'pdf',
      status: 'no_text',
      pageCount: 2,
      textPageCount: 0,
      includedPageCount: 0,
      omissions: [],
    }),
  ).toBe(
    'No PDF text layer text found · 0 of 2 pages with text · 0 pages included',
  );
  expect(
    describeDocumentExtraction({
      version: 2,
      format: 'pdf',
      status: 'context_omitted',
      pageCount: null,
      textPageCount: null,
      includedPageCount: null,
      omissions: ['context_limit'],
    }),
  ).toBe(
    'PDF text not included · Not all PDF text fit in this message context',
  );
});

it('quietly rejects invalid or historical PDF summaries', () => {
  const valid = {
    version: 2,
    format: 'pdf',
    status: 'ready',
    pageCount: 2,
    textPageCount: 1,
    includedPageCount: 1,
    omissions: [],
  };
  const invalid = [
    undefined,
    { ...valid, version: 1 },
    { ...valid, status: 'future_status' },
    { ...valid, privateText: 'secret' },
    { ...valid, page: 1 },
    { ...valid, omissions: ['main_body_only'] },
    { ...valid, omissions: ['text_layer_only', 'text_layer_only'] },
    { ...valid, pageCount: null },
    { ...valid, textPageCount: null },
    { ...valid, includedPageCount: null },
    { ...valid, pageCount: 101 },
    { ...valid, textPageCount: 3 },
    { ...valid, includedPageCount: 2 },
    { ...valid, status: 'ready', includedPageCount: 0 },
    { ...valid, status: 'no_text', textPageCount: 1 },
    { ...valid, status: 'no_text', pageCount: 0 },
    { ...valid, status: 'context_omitted', includedPageCount: 1 },
    {
      ...valid,
      status: 'malformed',
      pageCount: 2,
      textPageCount: 1,
      includedPageCount: 0,
    },
    {
      ...valid,
      status: 'malformed',
      pageCount: null,
      textPageCount: 2,
      includedPageCount: null,
    },
    { ...valid, status: 'partial', textPageCount: 0, includedPageCount: 0 },
  ];

  for (const extraction of invalid) {
    expect(describeDocumentExtraction(extraction)).toBeNull();
  }
});

it('keeps known PDF coverage visible when context omits all text', () => {
  expect(
    describeDocumentExtraction({
      version: 2,
      format: 'pdf',
      status: 'context_omitted',
      pageCount: 2,
      textPageCount: 1,
      includedPageCount: 0,
      omissions: ['text_layer_only', 'context_limit'],
    }),
  ).toBe(
    'PDF text not included · 1 of 2 pages with text · 0 pages included · Only the PDF text layer was read · Not all PDF text fit in this message context',
  );
});

it('renders a PDF summary beside its unchanged original download', async () => {
  const download = vi
    .spyOn(daemon, 'downloadAttachment')
    .mockResolvedValue(new Blob(['pdf'], { type: 'application/pdf' }));
  render(
    <MessageAttachments
      agentId="agent"
      attachments={[
        {
          ...uploaded,
          type: 'file',
          kind: 'document',
          mime: 'application/pdf',
          name: 'selected.pdf',
          extraction: {
            version: 2,
            format: 'pdf',
            status: 'partial',
            pageCount: 2,
            textPageCount: 1,
            includedPageCount: 1,
            omissions: ['text_layer_only'],
          },
        },
      ]}
    />,
  );

  expect(screen.getByText(/Some PDF text layer extracted/)).toHaveTextContent(
    'Some PDF text layer extracted · 1 of 2 pages with text · 1 page included · Only the PDF text layer was read',
  );
  await userEvent.click(
    screen.getByRole('button', { name: 'Download selected.pdf' }),
  );
  expect(download).toHaveBeenCalledWith('agent', 'att_1', {
    signal: expect.any(AbortSignal),
  });
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

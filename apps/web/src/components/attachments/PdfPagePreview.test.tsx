import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { daemon } from '../../lib/daemon-api';
import { PdfPagePreview } from './PdfPagePreview';

const source = {
  kind: 'attachment' as const,
  agentId: 'agent',
  attachmentId: 'att_one',
  pageCount: 3,
};
const png = () => new Blob(['page'], { type: 'image/png' });

beforeEach(() => {
  vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:pdf-page');
  vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
});
afterEach(() => vi.restoreAllMocks());

it('waits for an explicit preview click and releases the authenticated image on close', async () => {
  const load = vi
    .spyOn(daemon, 'previewAttachmentPage')
    .mockResolvedValue(png());
  render(<PdfPagePreview source={source} />);
  expect(load).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'Preview PDF page 1' }));
  const image = await screen.findByRole('img', { name: 'Original PDF page 1' });
  expect(load).toHaveBeenCalledWith('agent', 'att_one', 1, {
    signal: expect.any(AbortSignal),
  });
  expect(image).toHaveAttribute('src', 'blob:pdf-page');
  fireEvent.click(screen.getByRole('button', { name: 'Close PDF preview' }));
  expect(screen.queryByRole('img')).not.toBeInTheDocument();
  expect(load.mock.calls[0][3]?.signal?.aborted).toBe(true);
  expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:pdf-page');
});

it('aborts an old page selection and ignores its late response', async () => {
  let resolve!: (blob: Blob) => void;
  const load = vi
    .spyOn(daemon, 'previewAttachmentPage')
    .mockImplementationOnce(
      () =>
        new Promise<Blob>((done) => {
          resolve = done;
        }),
    )
    .mockResolvedValue(png());
  render(<PdfPagePreview source={source} />);
  fireEvent.click(screen.getByRole('button', { name: 'Preview PDF page 1' }));
  expect(screen.getByRole('status')).toHaveTextContent('Loading PDF page 1');
  fireEvent.change(screen.getByRole('combobox', { name: 'PDF page' }), {
    target: { value: '2' },
  });
  expect(load.mock.calls[0][3]?.signal?.aborted).toBe(true);
  await act(async () => resolve(png()));
  expect(URL.createObjectURL).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'Preview PDF page 2' }));
  await screen.findByRole('img', { name: 'Original PDF page 2' });
  expect(load).toHaveBeenLastCalledWith('agent', 'att_one', 2, {
    signal: expect.any(AbortSignal),
  });
});

it('clears the image and aborts pending work when the source scope changes', async () => {
  const load = vi
    .spyOn(daemon, 'previewAttachmentPage')
    .mockResolvedValue(png());
  const view = render(<PdfPagePreview source={source} />);
  fireEvent.click(screen.getByRole('button', { name: 'Preview PDF page 1' }));
  await screen.findByRole('img');
  view.rerender(<PdfPagePreview source={{ ...source, agentId: 'other' }} />);
  expect(screen.queryByRole('img')).not.toBeInTheDocument();
  expect(load.mock.calls[0][3]?.signal?.aborted).toBe(true);
  expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:pdf-page');
  expect(load).toHaveBeenCalledOnce();
});

it('shows a safe retry when loading or image decoding fails', async () => {
  vi.spyOn(daemon, 'previewAttachmentPage')
    .mockRejectedValueOnce(new Error('token=secret /private/path'))
    .mockResolvedValue(png());
  render(<PdfPagePreview source={source} />);
  fireEvent.click(screen.getByRole('button', { name: 'Preview PDF page 1' }));
  expect(await screen.findByRole('alert')).toHaveTextContent(
    'This PDF page could not be previewed. Try again.',
  );
  expect(screen.queryByText(/token=secret/)).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'Retry PDF preview' }));
  const image = await screen.findByRole('img');
  fireEvent.error(image);
  expect(screen.queryByRole('img')).not.toBeInTheDocument();
  expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:pdf-page');
  expect(
    screen.getByRole('button', { name: 'Retry PDF preview' }),
  ).toBeVisible();
});

it('aborts on unmount and never creates a URL for a late result', async () => {
  let resolve!: (blob: Blob) => void;
  const load = vi.spyOn(daemon, 'previewAttachmentPage').mockImplementation(
    () =>
      new Promise<Blob>((done) => {
        resolve = done;
      }),
  );
  const view = render(<PdfPagePreview source={source} />);
  fireEvent.click(screen.getByRole('button', { name: 'Preview PDF page 1' }));
  view.unmount();
  expect(load.mock.calls[0][3]?.signal?.aborted).toBe(true);
  await act(async () => resolve(png()));
  expect(URL.createObjectURL).not.toHaveBeenCalled();
});

it('resolves cited pages through the project client with the current scope', async () => {
  const api = {
    previewKnowledgeCitationPage: vi.fn().mockResolvedValue(png()),
  };
  const cited = {
    kind: 'citation' as const,
    agentId: 'agent',
    projectId: 'project',
    citationId: 'cite_one',
    page: 12,
    projectRevision: 4,
    accessEpoch: 2,
    memberGeneration: 3,
    api,
  };
  const view = render(<PdfPagePreview source={cited} />);
  expect(screen.queryByRole('combobox')).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'Preview PDF page 12' }));
  await screen.findByRole('img', { name: 'Original PDF page 12' });
  expect(api.previewKnowledgeCitationPage).toHaveBeenCalledWith(
    'project',
    'cite_one',
    'agent',
    { signal: expect.any(AbortSignal) },
  );
  view.rerender(<PdfPagePreview source={{ ...cited, accessEpoch: 3 }} />);
  expect(screen.queryByRole('img')).not.toBeInTheDocument();
  await waitFor(() =>
    expect(
      api.previewKnowledgeCitationPage.mock.calls[0][3].signal.aborted,
    ).toBe(true),
  );
});

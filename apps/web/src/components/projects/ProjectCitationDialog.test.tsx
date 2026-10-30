import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ProjectCitationDialog } from './ProjectCitationDialog';

const citation = {
  projectRevision: 4,
  accessEpoch: 2,
  memberGeneration: 3,
  citationId: 'cite_one',
  resourceId: 'source',
  documentId: 'document',
  title: 'Report.pdf',
  locator: 'pdf#page=12;byte=0',
  excerpt: 'Evidence',
  text: 'Verified passage',
};

beforeEach(() => {
  vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:citation-page');
  vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
});
afterEach(() => vi.restoreAllMocks());

it('labels OCR as recognized text that may contain errors instead of a verified literal passage', () => {
  render(
    <ProjectCitationDialog
      citation={{
        ...citation,
        recognition: {
          kind: 'ocr',
          language: 'en',
          pipeline: 'windows-ocr-v1',
        },
      }}
      opening={false}
      onClose={vi.fn()}
    />,
  );
  expect(screen.getByText('Recognized text')).toBeVisible();
  expect(
    screen.getByText(
      'Text recognition may contain errors. Check the original page.',
    ),
  ).toBeVisible();
  expect(screen.getByText('Verified passage')).toBeVisible();
  expect(
    screen.queryByText(
      'Bounded passage verified against the current source when opened.',
    ),
  ).not.toBeInTheDocument();
});

it('offers page-image evidence without inventing a recognized passage', async () => {
  const api = {
    previewKnowledgeCitationPage: vi
      .fn()
      .mockResolvedValue(new Blob(['page'], { type: 'image/png' })),
  };
  render(
    <ProjectCitationDialog
      citation={{
        ...citation,
        text: '',
        recognition: { kind: 'page_image', pipeline: 'windows-pdf-v1' },
      }}
      opening={false}
      onClose={vi.fn()}
      pageScope={{ agentId: 'agent', projectId: 'current_project', api }}
    />,
  );
  expect(screen.getByText('Page evidence')).toBeVisible();
  expect(
    screen.getByText(
      'No recognized passage is available. Open the original page to inspect it.',
    ),
  ).toBeVisible();
  expect(screen.getByRole('dialog').querySelector('pre')).toBeNull();
  expect(
    screen.queryByText(
      'Bounded passage verified against the current source when opened.',
    ),
  ).not.toBeInTheDocument();
  expect(api.previewKnowledgeCitationPage).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'Preview PDF page 12' }));
  await screen.findByRole('img', { name: 'Original PDF page 12' });
});

it('retains the literal-passage label when recognition provenance is absent', () => {
  render(
    <ProjectCitationDialog
      citation={citation}
      opening={false}
      onClose={vi.fn()}
    />,
  );
  expect(
    screen.getByText(
      'Bounded passage verified against the current source when opened.',
    ),
  ).toBeVisible();
});

it('offers the original PDF page in the verified citation dialog without loading it automatically', async () => {
  const api = {
    previewKnowledgeCitationPage: vi
      .fn()
      .mockResolvedValue(new Blob(['page'], { type: 'image/png' })),
  };
  render(
    <ProjectCitationDialog
      citation={citation}
      opening={false}
      onClose={vi.fn()}
      pageScope={{ agentId: 'agent', projectId: 'current_project', api }}
    />,
  );
  expect(screen.getByText('Verified passage')).toBeVisible();
  expect(api.previewKnowledgeCitationPage).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'Preview PDF page 12' }));
  await screen.findByRole('img', { name: 'Original PDF page 12' });
  expect(api.previewKnowledgeCitationPage).toHaveBeenCalledWith(
    'current_project',
    'cite_one',
    'agent',
    { signal: expect.any(AbortSignal) },
  );
});

it.each([
  'Page 12',
  'pdf#page=0',
  'pdf#page=101',
  'https://example.test/page=12',
  'pdf#page=2extra',
])('does not invent a PDF page from locator %s', (locator) => {
  const api = { previewKnowledgeCitationPage: vi.fn() };
  render(
    <ProjectCitationDialog
      citation={{ ...citation, locator }}
      opening={false}
      onClose={vi.fn()}
      pageScope={{ agentId: 'agent', projectId: 'current_project', api }}
    />,
  );
  expect(
    screen.queryByRole('button', { name: /Preview PDF/ }),
  ).not.toBeInTheDocument();
});

it('removes pending page evidence when citation access is being checked again', async () => {
  let resolve!: (blob: Blob) => void;
  const api = {
    previewKnowledgeCitationPage: vi.fn().mockImplementation(
      () =>
        new Promise<Blob>((done) => {
          resolve = done;
        }),
    ),
  };
  const pageScope = { agentId: 'agent', projectId: 'current_project', api };
  const view = render(
    <ProjectCitationDialog
      citation={citation}
      opening={false}
      onClose={vi.fn()}
      pageScope={pageScope}
    />,
  );
  fireEvent.click(screen.getByRole('button', { name: 'Preview PDF page 12' }));
  view.rerender(
    <ProjectCitationDialog
      citation={citation}
      opening={true}
      onClose={vi.fn()}
      pageScope={pageScope}
    />,
  );
  expect(api.previewKnowledgeCitationPage.mock.calls[0][3].signal.aborted).toBe(
    true,
  );
  await act(async () => resolve(new Blob(['late'], { type: 'image/png' })));
  expect(screen.queryByRole('img')).not.toBeInTheDocument();
  expect(URL.createObjectURL).not.toHaveBeenCalled();
});

import type { ProjectKnowledgeBinding, ProjectsClient } from '@animaos/sdk';
import { useEffect, useRef, useState } from 'react';
import { useLatest } from '../../hooks/useLatest';
import { useRenderScope } from '../../hooks/useRenderScope';
import { daemon } from '../../lib/daemon-api';
import './PdfPagePreview.css';

export type CitationPageScope = {
  agentId: string;
  projectId: string;
  api: Pick<ProjectsClient, 'previewKnowledgeCitationPage'>;
};

export type PdfPagePreviewSource =
  | {
      kind: 'attachment';
      agentId: string;
      attachmentId: string;
      pageCount: number;
    }
  | {
      kind: 'attachment_page';
      agentId: string;
      attachmentId: string;
      page: number;
    }
  | (CitationPageScope &
      ProjectKnowledgeBinding & {
        kind: 'citation';
        citationId: string;
        page: number;
      });

type PreviewState =
  | { phase: 'closed' }
  | { phase: 'loading' }
  | { phase: 'ready'; url: string }
  | { phase: 'error' };

const PREVIEW_ERROR = 'This PDF page could not be previewed. Try again.';

export function PdfPagePreview({
  source,
  openOnMount = false,
}: {
  source: PdfPagePreviewSource;
  openOnMount?: boolean;
}) {
  const scope = useRenderScope(
    source.kind,
    source.agentId,
    source.kind === 'citation' ? source.projectId : source.attachmentId,
    source.kind === 'attachment'
      ? source.pageCount
      : source.kind === 'citation'
        ? source.citationId
        : source.page,
    source.kind !== 'attachment' ? source.page : null,
    source.kind === 'citation' ? source.projectRevision : null,
    source.kind === 'citation' ? source.accessEpoch : null,
    source.kind === 'citation' ? source.memberGeneration : null,
    source.kind === 'citation' ? source.api : null,
  );
  const lastPage =
    source.kind === 'attachment' ? source.pageCount : source.page;
  if (!Number.isInteger(lastPage) || lastPage < 1 || lastPage > 100)
    return null;
  return (
    <PagePreview
      key={scope.generation}
      source={source}
      openOnMount={openOnMount}
    />
  );
}

function PagePreview({
  source,
  openOnMount,
}: {
  source: PdfPagePreviewSource;
  openOnMount: boolean;
}) {
  const [page, setPage] = useState(
    source.kind === 'attachment' ? 1 : source.page,
  );
  const [state, setState] = useState<PreviewState>({
    phase: openOnMount ? 'loading' : 'closed',
  });
  const [attempt, setAttempt] = useState(0);
  const latestSource = useLatest(source);
  const owner = useRef<{
    active: boolean;
    controller: AbortController;
    url: string | null;
  } | null>(null);
  const opened = state.phase !== 'closed';
  const request = useRenderScope(opened, page, attempt);
  const requestBoundary = useLatest(request);
  const generation = request.generation;
  useEffect(() => {
    if (!opened) return;
    const current = {
      active: true,
      controller: new AbortController(),
      url: null as string | null,
    };
    owner.current = current;
    const active = () =>
      current.active &&
      !current.controller.signal.aborted &&
      requestBoundary.current.generation === generation;
    const selected = latestSource.current;
    const load =
      selected.kind === 'citation'
        ? selected.api.previewKnowledgeCitationPage(
            selected.projectId,
            selected.citationId,
            selected.agentId,
            { signal: current.controller.signal },
          )
        : daemon.previewAttachmentPage(
            selected.agentId,
            selected.attachmentId,
            page,
            { signal: current.controller.signal },
          );
    void load
      .then((blob) => {
        if (!active()) return;
        if (
          blob.type !== 'image/png' ||
          blob.size === 0 ||
          blob.size > 2 * 1024 * 1024
        ) {
          setState({ phase: 'error' });
          return;
        }
        current.url = URL.createObjectURL(blob);
        setState({ phase: 'ready', url: current.url });
      })
      .catch(() => {
        if (active()) setState({ phase: 'error' });
      });
    return () => {
      current.active = false;
      current.controller.abort();
      if (current.url) URL.revokeObjectURL(current.url);
      current.url = null;
    };
  }, [opened, page, generation, latestSource, requestBoundary]);

  const imageFailed = () => {
    const current = owner.current;
    if (
      !current?.active ||
      current.controller.signal.aborted ||
      requestBoundary.current.generation !== generation
    )
      return;
    if (current.url) URL.revokeObjectURL(current.url);
    current.url = null;
    current.controller.abort();
    setState({ phase: 'error' });
  };

  return (
    <div className="pdf-page-preview">
      <div className="pdf-page-preview-controls">
        {source.kind === 'attachment' && source.pageCount > 1 && (
          <label>
            PDF page
            <select
              value={page}
              onChange={(event) => {
                setPage(Number(event.target.value));
                setState({ phase: 'closed' });
              }}
            >
              {Array.from({ length: source.pageCount }, (_, index) => (
                <option key={index + 1} value={index + 1}>
                  {index + 1}
                </option>
              ))}
            </select>
          </label>
        )}
        {opened ? (
          <button type="button" onClick={() => setState({ phase: 'closed' })}>
            Close PDF preview
          </button>
        ) : (
          <button type="button" onClick={() => setState({ phase: 'loading' })}>
            Preview PDF page {page}
          </button>
        )}
      </div>
      {state.phase === 'loading' && (
        <p role="status">Loading PDF page {page}…</p>
      )}
      {state.phase === 'error' && (
        <div role="alert">
          <p>{PREVIEW_ERROR}</p>
          <button
            type="button"
            onClick={() => {
              setState({ phase: 'loading' });
              setAttempt((value) => value + 1);
            }}
          >
            Retry PDF preview
          </button>
        </div>
      )}
      {state.phase === 'ready' && (
        <figure>
          <img
            src={state.url}
            alt={`Original PDF page ${page}`}
            onError={imageFailed}
          />
          <figcaption>Original PDF page {page}</figcaption>
        </figure>
      )}
    </div>
  );
}

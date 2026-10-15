import { useEffect, useRef, useState } from 'react';
import type { SessionMessageAttachment } from '@animaOS-SWARM/sdk';
import { useAttachmentPreview } from '../../hooks/useAttachmentPreview';
import { daemon } from '../../lib/daemon-api';
import {
  ATTACHMENT_ERRORS,
  IMAGE_ATTACHMENT_MIMES,
} from '../../lib/attachments';
import { revealInvisible } from '../../lib/skills';
import { formatAttachmentSize } from './AttachmentChips';

const EXTRACTION_STATUS_LABELS = {
  ready: 'Main body text extracted',
  partial: 'Some main body text extracted',
  unsupported: 'Text extraction unsupported',
  encrypted: 'Encrypted document',
  malformed: 'Document could not be read',
  too_large: 'Document too large for text extraction',
  no_text: 'No main body text found',
  unavailable: 'Text extraction unavailable',
  cancelled: 'Text extraction cancelled',
  context_omitted: 'Document text not included',
} as const;

const EXTRACTION_OMISSION_LABELS = {
  main_body_only: 'Only main body text was read',
  paragraph_limit: 'Paragraph limit reached',
  text_limit: 'Text limit reached',
  context_limit: 'Not all text fit in this message context',
  selection_limit: 'Additional documents were not included',
} as const;

const MAX_EXTRACTION_PARAGRAPHS = 2000;

export function describeDocumentExtraction(value: unknown): string | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  const extraction = value as {
    version?: unknown;
    status?: unknown;
    paragraphCount?: unknown;
    omissions?: unknown;
  };
  if (
    extraction.version !== 1 ||
    typeof extraction.status !== 'string' ||
    !Object.prototype.hasOwnProperty.call(
      EXTRACTION_STATUS_LABELS,
      extraction.status,
    ) ||
    !Number.isInteger(extraction.paragraphCount) ||
    !Array.isArray(extraction.omissions) ||
    extraction.omissions.length > 5 ||
    new Set(extraction.omissions).size !== extraction.omissions.length ||
    !extraction.omissions.every(
      (omission) =>
        typeof omission === 'string' &&
        Object.prototype.hasOwnProperty.call(
          EXTRACTION_OMISSION_LABELS,
          omission,
        ),
    )
  ) {
    return null;
  }

  const paragraphCount = extraction.paragraphCount as number;
  const hasExtractedText =
    extraction.status === 'ready' || extraction.status === 'partial';
  if (
    (hasExtractedText &&
      (paragraphCount < 1 || paragraphCount > MAX_EXTRACTION_PARAGRAPHS)) ||
    (!hasExtractedText && paragraphCount !== 0)
  ) {
    return null;
  }

  const status =
    EXTRACTION_STATUS_LABELS[
      extraction.status as keyof typeof EXTRACTION_STATUS_LABELS
    ];
  const details: string[] = [status];
  if (hasExtractedText) {
    details.push(
      `${paragraphCount} ${paragraphCount === 1 ? 'paragraph' : 'paragraphs'}`,
    );
  }
  for (const omission of extraction.omissions as string[]) {
    details.push(
      EXTRACTION_OMISSION_LABELS[
        omission as keyof typeof EXTRACTION_OMISSION_LABELS
      ],
    );
  }
  return details.join(' · ');
}

function AttachmentFile({
  agentId,
  attachment,
}: {
  agentId: string;
  attachment: SessionMessageAttachment;
}) {
  const name = revealInvisible(attachment.name).text;
  const image =
    attachment.type === 'image' &&
    Boolean(attachment.mime && IMAGE_ATTACHMENT_MIMES.has(attachment.mime));
  const extractionDescription = describeDocumentExtraction(
    attachment.kind === 'document' ? attachment.extraction : undefined,
  );
  const preview = useAttachmentPreview(
    agentId,
    attachment.id,
    attachment.mime,
    image,
  );
  const [error, setError] = useState<string | null>(null);
  const [downloading, setDownloading] = useState(false);
  const boundary = useRef({ agentId, id: attachment.id, generation: 0 });
  if (
    boundary.current.agentId !== agentId ||
    boundary.current.id !== attachment.id
  )
    boundary.current = {
      agentId,
      id: attachment.id,
      generation: boundary.current.generation + 1,
    };
  const generation = boundary.current.generation;
  const mounted = useRef(true);
  const requests = useRef(new Set<AbortController>());
  const urls = useRef(new Map<string, number>());
  useEffect(() => {
    mounted.current = true;
    setError(null);
    setDownloading(false);
    return () => {
      mounted.current = false;
      for (const controller of requests.current) controller.abort();
      requests.current.clear();
      for (const [url, timer] of urls.current) {
        window.clearTimeout(timer);
        URL.revokeObjectURL(url);
      }
      urls.current.clear();
    };
  }, [agentId, attachment.id]);
  const download = async () => {
    if (!attachment.id || downloading) return;
    const controller = new AbortController();
    requests.current.add(controller);
    setDownloading(true);
    setError(null);
    const active = () =>
      mounted.current &&
      boundary.current.generation === generation &&
      !controller.signal.aborted;
    try {
      const blob = await daemon.downloadAttachment(agentId, attachment.id, {
        signal: controller.signal,
      });
      if (!active()) return;
      const url = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = url;
      link.download = attachment.name;
      const timer = window.setTimeout(() => {
        URL.revokeObjectURL(url);
        urls.current.delete(url);
      }, 0);
      urls.current.set(url, timer);
      link.click();
    } catch {
      if (active()) setError(ATTACHMENT_ERRORS.download);
    } finally {
      requests.current.delete(controller);
      if (active()) setDownloading(false);
    }
  };
  return (
    <li className="studio-message-attachment">
      {preview.url && (
        <img
          src={preview.url}
          alt={name}
          onError={preview.imageFailed}
          className="studio-attachment-image"
        />
      )}
      <span>
        <strong>{name}</strong>
        <small>
          {attachment.kind ?? attachment.type}
          {attachment.size !== undefined
            ? ` · ${formatAttachmentSize(attachment.size)}`
            : ''}
          {extractionDescription ? ` · ${extractionDescription}` : ''}
        </small>
      </span>
      {attachment.id && (
        <button
          type="button"
          disabled={downloading}
          aria-label={`Download ${name}`}
          onClick={() => void download()}
        >
          {downloading ? 'Downloading…' : 'Download'}
        </button>
      )}
      {preview.loading && <span role="status">Loading preview…</span>}
      {preview.error && (
        <span role="status">
          {preview.error}{' '}
          <button type="button" onClick={preview.retry}>
            Retry preview
          </button>
        </span>
      )}
      {error && <span role="alert">{error}</span>}
    </li>
  );
}
export function MessageAttachments({
  agentId,
  attachments,
}: {
  agentId: string;
  attachments: readonly SessionMessageAttachment[];
}) {
  if (!attachments.length) return null;
  return (
    <ul className="studio-message-attachments" aria-label="Attached files">
      {attachments.map((attachment, index) => (
        <AttachmentFile
          key={`${attachment.id ?? 'legacy'}:${index}`}
          agentId={agentId}
          attachment={attachment}
        />
      ))}
    </ul>
  );
}

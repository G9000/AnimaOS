import { useResetOnChange } from '../../hooks/useResetOnChange';
import { useRenderScope } from '../../hooks/useRenderScope';
import { useLatest } from '../../hooks/useLatest';
import { useEffect, useRef, useState } from 'react';
import type { SessionMessageAttachment } from '@animaos/sdk';
import { useAttachmentPreview } from '../../hooks/useAttachmentPreview';
import { daemon } from '../../lib/daemon-api';
import {
  ATTACHMENT_ERRORS,
  IMAGE_ATTACHMENT_MIMES,
} from '../../lib/attachments';
import { revealInvisible } from '../../lib/skills';
import { formatAttachmentSize } from './AttachmentChips';
import { PdfPagePreview } from './PdfPagePreview';

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

const PDF_EXTRACTION_STATUS_LABELS = {
  ready: 'PDF text layer extracted',
  partial: 'Some PDF text layer extracted',
  unsupported: 'PDF text extraction unsupported',
  encrypted: 'Encrypted PDF',
  malformed: 'PDF could not be read',
  too_large: 'PDF too large for text extraction',
  no_text: 'No PDF text layer text found',
  unavailable: 'PDF text extraction unavailable',
  cancelled: 'PDF text extraction cancelled',
  context_omitted: 'PDF text not included',
} as const;

const PDF_EXTRACTION_OMISSION_LABELS = {
  text_layer_only: 'Only the PDF text layer was read',
  text_limit: 'PDF text limit reached',
  context_limit: 'Not all PDF text fit in this message context',
  selection_limit: 'Additional documents were not included',
} as const;

const XLSX_EXTRACTION_STATUS_LABELS = {
  partial: 'Some XLSX stored cells extracted',
  unsupported: 'XLSX text extraction unsupported',
  encrypted: 'Encrypted XLSX workbook',
  malformed: 'XLSX workbook could not be read',
  too_large: 'XLSX workbook too large for text extraction',
  no_text: 'No readable XLSX stored cells found',
  unavailable: 'XLSX text extraction unavailable',
  cancelled: 'XLSX text extraction cancelled',
  context_omitted: 'XLSX cell text not included',
} as const;

const XLSX_EXTRACTION_OMISSION_LABELS = {
  stored_cells_only:
    'Stored cells only; formulas were not evaluated and cached values may be stale',
  cell_limit: 'Cell limit reached',
  cell_text_limit: 'Cell text limit reached',
  text_limit: 'XLSX text limit reached',
  context_limit: 'Not all XLSX cell text fit in this message context',
  selection_limit: 'Additional documents were not included',
} as const;

const MAX_EXTRACTION_PARAGRAPHS = 2000;
const MAX_PDF_PAGES = 100;
const MAX_XLSX_SHEETS = 16;
const MAX_XLSX_CELLS = 2000;

function hasExactKeys(value: object, keys: readonly string[]): boolean {
  const actual = Object.keys(value).sort();
  const expected = [...keys].sort();
  return (
    actual.length === expected.length &&
    actual.every((key, index) => key === expected[index])
  );
}

function isClosedOmissionList(
  value: unknown,
  labels: Record<string, string>,
): value is string[] {
  return (
    Array.isArray(value) &&
    value.length <= 5 &&
    new Set(value).size === value.length &&
    value.every(
      (omission) =>
        typeof omission === 'string' &&
        Object.prototype.hasOwnProperty.call(labels, omission),
    )
  );
}

export function describeDocumentExtraction(value: unknown): string | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  if ('format' in value && (value as { format?: unknown }).format === 'xlsx') {
    return describeXlsxExtraction(value);
  }
  if ('format' in value && (value as { format?: unknown }).format === 'pdf') {
    const extraction = value as {
      version?: unknown;
      format?: unknown;
      status?: unknown;
      pageCount?: unknown;
      textPageCount?: unknown;
      includedPageCount?: unknown;
      omissions?: unknown;
    };
    if (
      !hasExactKeys(value, [
        'version',
        'format',
        'status',
        'pageCount',
        'textPageCount',
        'includedPageCount',
        'omissions',
      ]) ||
      extraction.version !== 2 ||
      extraction.format !== 'pdf' ||
      typeof extraction.status !== 'string' ||
      !Object.prototype.hasOwnProperty.call(
        PDF_EXTRACTION_STATUS_LABELS,
        extraction.status,
      ) ||
      !isClosedOmissionList(
        extraction.omissions,
        PDF_EXTRACTION_OMISSION_LABELS,
      )
    ) {
      return null;
    }

    const { pageCount, textPageCount, includedPageCount } = extraction;
    const status =
      extraction.status as keyof typeof PDF_EXTRACTION_STATUS_LABELS;
    const allNull =
      pageCount === null &&
      textPageCount === null &&
      includedPageCount === null;
    const knownCounts = [pageCount, textPageCount, includedPageCount].every(
      (count) =>
        Number.isInteger(count) &&
        (count as number) >= 0 &&
        (count as number) <= MAX_PDF_PAGES,
    );
    const countFailure =
      status === 'unsupported' ||
      status === 'encrypted' ||
      status === 'malformed' ||
      status === 'too_large' ||
      status === 'unavailable' ||
      status === 'cancelled';
    if (
      countFailure
        ? !allNull
        : !(knownCounts || (status === 'context_omitted' && allNull))
    ) {
      return null;
    }

    if (knownCounts) {
      const pages = pageCount as number;
      const textPages = textPageCount as number;
      const includedPages = includedPageCount as number;
      if (
        includedPages > textPages ||
        textPages > pages ||
        (pages === 0 &&
          !(status === 'no_text' && textPages === 0 && includedPages === 0)) ||
        ((status === 'ready' || status === 'partial') &&
          (pages < 1 || textPages < 1 || includedPages < 1)) ||
        (status === 'no_text' && (textPages !== 0 || includedPages !== 0)) ||
        (status === 'context_omitted' && includedPages !== 0)
      ) {
        return null;
      }
    }

    const details: string[] = [
      PDF_EXTRACTION_STATUS_LABELS[
        status as keyof typeof PDF_EXTRACTION_STATUS_LABELS
      ],
    ];
    if (knownCounts) {
      const pages = pageCount as number;
      const textPages = textPageCount as number;
      const includedPages = includedPageCount as number;
      details.push(
        `${textPages} of ${pages} ${pages === 1 ? 'page' : 'pages'} with text`,
        `${includedPages} ${includedPages === 1 ? 'page' : 'pages'} included`,
      );
    }
    for (const omission of extraction.omissions) {
      details.push(
        PDF_EXTRACTION_OMISSION_LABELS[
          omission as keyof typeof PDF_EXTRACTION_OMISSION_LABELS
        ],
      );
    }
    return details.join(' · ');
  }

  const extraction = value as {
    version?: unknown;
    status?: unknown;
    paragraphCount?: unknown;
    omissions?: unknown;
  };
  if (
    !hasExactKeys(value, [
      'version',
      'status',
      'paragraphCount',
      'omissions',
    ]) ||
    extraction.version !== 1 ||
    typeof extraction.status !== 'string' ||
    !Object.prototype.hasOwnProperty.call(
      EXTRACTION_STATUS_LABELS,
      extraction.status,
    ) ||
    !Number.isInteger(extraction.paragraphCount) ||
    !isClosedOmissionList(extraction.omissions, EXTRACTION_OMISSION_LABELS)
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

function describeXlsxExtraction(value: object): string | null {
  const extraction = value as {
    version?: unknown;
    format?: unknown;
    status?: unknown;
    sheetCount?: unknown;
    observedCellCount?: unknown;
    includedCellCount?: unknown;
    hiddenSheetCount?: unknown;
    omissions?: unknown;
  };
  if (
    !hasExactKeys(value, [
      'version',
      'format',
      'status',
      'sheetCount',
      'observedCellCount',
      'includedCellCount',
      'hiddenSheetCount',
      'omissions',
    ]) ||
    extraction.version !== 2 ||
    extraction.format !== 'xlsx' ||
    typeof extraction.status !== 'string' ||
    !Object.prototype.hasOwnProperty.call(
      XLSX_EXTRACTION_STATUS_LABELS,
      extraction.status,
    ) ||
    !isClosedOmissionList(extraction.omissions, XLSX_EXTRACTION_OMISSION_LABELS)
  ) {
    return null;
  }

  const status =
    extraction.status as keyof typeof XLSX_EXTRACTION_STATUS_LABELS;
  const { sheetCount, observedCellCount, includedCellCount, hiddenSheetCount } =
    extraction;
  const counts = [
    sheetCount,
    observedCellCount,
    includedCellCount,
    hiddenSheetCount,
  ];
  const allNull = counts.every((count) => count === null);
  const knownCounts = counts.every(
    (count, index) =>
      Number.isInteger(count) &&
      (count as number) >= 0 &&
      (count as number) <=
        (index === 0 || index === 3 ? MAX_XLSX_SHEETS : MAX_XLSX_CELLS),
  );
  if (
    (!allNull && !knownCounts) ||
    ((status === 'partial' || status === 'no_text') && !knownCounts) ||
    (status === 'partial' &&
      !extraction.omissions.includes('stored_cells_only'))
  ) {
    return null;
  }

  const details: string[] = [XLSX_EXTRACTION_STATUS_LABELS[status]];
  if (knownCounts) {
    const sheets = sheetCount as number;
    const observed = observedCellCount as number;
    const included = includedCellCount as number;
    const hidden = hiddenSheetCount as number;
    if (
      included > observed ||
      hidden > sheets ||
      (sheets === 0 && observed !== 0) ||
      (status === 'partial' ? included === 0 : included !== 0)
    ) {
      return null;
    }
    details.push(
      `${sheets} ${sheets === 1 ? 'sheet' : 'sheets'}`,
      `${included} of ${observed} cells included`,
      `${hidden} hidden ${hidden === 1 ? 'sheet' : 'sheets'}`,
    );
  }
  for (const omission of extraction.omissions) {
    details.push(
      XLSX_EXTRACTION_OMISSION_LABELS[
        omission as keyof typeof XLSX_EXTRACTION_OMISSION_LABELS
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
  const pdfPages =
    attachment.id && attachment.kind === 'document' && attachment.mime === 'application/pdf' &&
    extractionDescription && attachment.extraction && 'format' in attachment.extraction &&
    attachment.extraction.format === 'pdf'
      ? attachment.extraction.pageCount
      : null;
  const preview = useAttachmentPreview(
    agentId,
    attachment.id,
    attachment.mime,
    image,
  );
  const [error, setError] = useState<string | null>(null);
  const [downloading, setDownloading] = useState(false);
  const scope = useRenderScope(agentId, attachment.id);
  const boundary = useLatest(scope);
  useResetOnChange([scope], () => {
    setError(null);
    setDownloading(false);
  });
  const mounted = useRef(true);
  const requests = useRef(new Set<AbortController>());
  const urls = useRef(new Map<string, number>());
  useEffect(() => {
    mounted.current = true;
    const ownedRequests = requests.current;
    const ownedUrls = urls.current;
    return () => {
      mounted.current = false;
      for (const controller of ownedRequests) controller.abort();
      ownedRequests.clear();
      for (const [url, timer] of ownedUrls) {
        window.clearTimeout(timer);
        URL.revokeObjectURL(url);
      }
      ownedUrls.clear();
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
      boundary.current === scope &&
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
      {attachment.id && pdfPages !== null && pdfPages > 0 && <PdfPagePreview source={{
        kind: 'attachment', agentId, attachmentId: attachment.id, pageCount: pdfPages,
      }} />}
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

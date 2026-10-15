import { DaemonHttpError, type UploadedAttachment } from '@animaOS-SWARM/sdk';
import { daemonRequestHeaders } from './daemon-api';

export const MAX_DRAFT_ATTACHMENTS = 10;
export const ATTACHMENT_ERRORS = {
  empty: 'This file is empty. Choose a file with content.',
  tooLarge:
    'This file is too large. Images allow 10 MiB, text 1 MiB, documents 25 MiB, and WAV audio 4 MiB.',
  unsupported:
    'This file type is not supported. Choose an image, text/source file, PDF, Office document, or PCM16 WAV audio (up to 120 seconds).',
  limit: 'You can attach at most 10 files to a message.',
  upload: 'The file could not upload. Remove it and try again.',
  invalidResponse: 'The daemon returned invalid attachment details. Try again.',
  cancelled: 'Upload cancelled. Remove the file and add it again.',
  preview: 'The image preview is unavailable. Retry or download the file.',
  download: 'The file could not download. Try again.',
  localCommand:
    'Remove the attachments before running a local slash command, or send them with a message.',
} as const;
export const IMAGE_ATTACHMENT_MIMES = new Set([
  'image/png',
  'image/jpeg',
  'image/webp',
  'image/gif',
]);
const TEXT_EXTENSIONS = new Set([
  'txt',
  'md',
  'markdown',
  'csv',
  'json',
  'yaml',
  'yml',
  'xml',
  'rs',
  'ts',
  'tsx',
  'js',
  'jsx',
  'py',
  'go',
  'java',
  'c',
  'h',
  'cpp',
  'hpp',
  'css',
  'sql',
  'toml',
  'sh',
]);
const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'webp', 'gif']);
const DOCUMENT_EXTENSIONS = new Set([
  'pdf',
  'doc',
  'xls',
  'ppt',
  'docx',
  'xlsx',
  'pptx',
]);

export interface AttachmentDraft {
  localId: string;
  name: string;
  kind?: UploadedAttachment['kind'];
  previewUrl?: string;
  status: 'uploading' | 'ready' | 'error';
  progress: number | null;
  attachment?: UploadedAttachment;
  /** A saved reference whose server display metadata is unavailable. */
  savedAttachmentId?: string;
  error?: string;
}

export function messageAttachment(attachment: UploadedAttachment) {
  return {
    ...attachment,
    type: attachment.kind === 'image' ? ('image' as const) : ('file' as const),
  };
}

export interface RecoveredAttachmentDraft {
  requestId: string;
  text: string;
  idempotencyKey?: string;
  attachmentIds?: string[];
  attachments?: UploadedAttachment[];
  skill?: string;
}
export function recoveredDraft(
  send: {
    key: string;
    text: string;
    attachmentIds?: string[];
    attachments?: UploadedAttachment[];
    skill?: string;
  },
  keepKey = true,
): RecoveredAttachmentDraft {
  return {
    requestId: send.key,
    text: send.text,
    ...(send.skill ? { skill: send.skill } : {}),
    ...(keepKey ? { idempotencyKey: send.key } : {}),
    ...(send.attachmentIds?.length
      ? {
          attachmentIds: [...send.attachmentIds],
          attachments: send.attachments ?? [],
        }
      : {}),
  };
}

export function classifyAttachmentFile(
  file: File,
):
  | { kind: UploadedAttachment['kind']; error?: never }
  | { error: string; kind?: never } {
  if (!file.size) return { error: ATTACHMENT_ERRORS.empty };
  const extension = file.name.split('.').at(-1)?.toLowerCase() ?? '';
  const kind = IMAGE_EXTENSIONS.has(extension)
    ? 'image'
    : TEXT_EXTENSIONS.has(extension)
      ? 'text'
      : DOCUMENT_EXTENSIONS.has(extension)
        ? 'document'
        : extension === 'wav'
          ? 'audio'
          : null;
  if (!kind) return { error: ATTACHMENT_ERRORS.unsupported };
  // The host validates the actual WAV container, PCM format and duration.
  const max =
    (kind === 'image' ? 10 : kind === 'text' ? 1 : kind === 'audio' ? 4 : 25) *
    1024 *
    1024;
  return file.size > max ? { error: ATTACHMENT_ERRORS.tooLarge } : { kind };
}

export function validAttachmentId(value: unknown): value is string {
  return (
    typeof value === 'string' &&
    value.length > 0 &&
    value.length <= 128 &&
    !/[\s\p{Cf}\p{Cc}]/u.test(value)
  );
}

/** Copies only bounded public fields: recovery never persists bytes or blob URLs. */
export function attachmentMetadata(value: unknown): UploadedAttachment | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  const item = value as Record<string, unknown>;
  if (
    !validAttachmentId(item.id) ||
    typeof item.name !== 'string' ||
    !item.name ||
    item.name.length > 255 ||
    typeof item.mime !== 'string' ||
    !item.mime ||
    item.mime.length > 128 ||
    !Number.isSafeInteger(item.size) ||
    (item.size as number) < 1 ||
    (item.size as number) > 25 * 1024 * 1024 ||
    !['image', 'text', 'document', 'audio'].includes(String(item.kind)) ||
    (item.kind === 'audio' &&
      (item.mime !== 'audio/wav' || (item.size as number) > 4 * 1024 * 1024)) ||
    typeof item.path !== 'string' ||
    !item.path ||
    item.path.length > 1024 ||
    /^[\\/]/.test(item.path) ||
    item.path.split(/[\\/]/).includes('..') ||
    !Number.isSafeInteger(item.createdAtMs) ||
    (item.createdAtMs as number) < 0
  )
    return null;
  return {
    id: item.id,
    name: item.name,
    mime: item.mime,
    size: item.size as number,
    kind: item.kind as UploadedAttachment['kind'],
    path: item.path,
    createdAtMs: item.createdAtMs as number,
  };
}

export function uploadAttachmentWithProgress(
  agentId: string,
  sessionId: string,
  file: File,
  {
    signal,
    onProgress,
  }: {
    signal?: AbortSignal;
    onProgress?: (percent: number | null) => void;
  } = {},
): Promise<UploadedAttachment> {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) {
      reject(new DOMException('Aborted', 'AbortError'));
      return;
    }
    const xhr = new XMLHttpRequest();
    let settled = false;
    const cleanup = () => {
      xhr.onload = null;
      xhr.onerror = null;
      xhr.onabort = null;
      xhr.upload.onprogress = null;
      signal?.removeEventListener('abort', onAbort);
    };
    const fail = (error: unknown) => {
      if (settled) return;
      settled = true;
      cleanup();
      reject(error);
    };
    const onAbort = () => {
      fail(new DOMException('Aborted', 'AbortError'));
      xhr.abort();
    };
    xhr.onload = () => {
      if (settled) return;
      let response: unknown;
      try {
        response = JSON.parse(xhr.responseText);
      } catch {
        response = xhr.responseText;
      }
      if (xhr.status < 200 || xhr.status >= 300) {
        fail(new DaemonHttpError(xhr.status, response));
        return;
      }
      const result = attachmentMetadata(
        response && typeof response === 'object'
          ? (response as { attachment?: unknown }).attachment
          : null,
      );
      if (!result) {
        fail(new Error(ATTACHMENT_ERRORS.invalidResponse));
        return;
      }
      settled = true;
      cleanup();
      resolve(result);
    };
    xhr.onerror = () => fail(new Error(ATTACHMENT_ERRORS.upload));
    xhr.onabort = () => fail(new DOMException('Aborted', 'AbortError'));
    xhr.upload.onprogress = (event) => {
      if (!settled)
        onProgress?.(
          event.lengthComputable && event.total > 0
            ? Math.min(100, Math.max(0, (event.loaded / event.total) * 100))
            : null,
        );
    };
    signal?.addEventListener('abort', onAbort, { once: true });
    try {
      xhr.open(
        'POST',
        `/api/agents/${encodeURIComponent(agentId)}/sessions/${encodeURIComponent(sessionId)}/attachments`,
      );
      for (const [name, value] of Object.entries({
        accept: 'application/json',
        ...daemonRequestHeaders(),
      }))
        xhr.setRequestHeader(name, value);
      const body = new FormData();
      body.append('file', file, file.name);
      xhr.send(body);
    } catch {
      fail(new Error(ATTACHMENT_ERRORS.upload));
    }
  });
}

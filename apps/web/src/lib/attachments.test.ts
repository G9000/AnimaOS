import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { DaemonHttpError } from '@animaos/sdk';
import {
  attachmentMetadata,
  classifyAttachmentFile,
  messageAttachment,
  uploadAttachmentWithProgress,
  ATTACHMENT_ERRORS,
  recoveredDraft,
} from './attachments';
import type { PreviewEditContext } from '@animaos/sdk';

it('preserves the exact public criteria and scope through failed-draft recovery', () => {
  const outcome = { criteria: [{ id: 'result', kind: 'response_text' as const, contains: 'Ready' }], maxCorrections: 1 };
  const recovered = recoveredDraft({ key: 'original', text: 'Prepare it', agentId: 'agent', sessionId: 'chat', outcome });
  outcome.criteria[0].contains = 'Changed after recovery';
  expect(recovered).toMatchObject({ idempotencyKey: 'original', agentId: 'agent', sessionId: 'chat', outcome: { criteria: [{ id: 'result', kind: 'response_text', contains: 'Ready' }], maxCorrections: 1 } });
});

it('preserves an exact document edit restriction and owner scope in a recoverable draft', () => {
  const documentEditContext: PreviewEditContext = { version: 1, source: { kind: 'workspace', path: 'report.html', sha256: 'a'.repeat(64), scope: 'd'.repeat(64) }, selection: { kind: 'html_text', start_utf8: 3, end_utf8: 8, expected_text: 'Hello' } };
  const recovered = recoveredDraft({ key: 'original', text: 'Rewrite', agentId: 'agent', sessionId: 'chat', documentEditContext });
  expect(recovered).toMatchObject({ requestId: 'original', idempotencyKey: 'original', agentId: 'agent', sessionId: 'chat', documentEditContext });
});

const metadata = {
  id: 'att_1',
  name: 'photo.png',
  mime: 'image/png',
  size: 3,
  kind: 'image',
  path: 'attachments/photo.png',
  createdAtMs: 1,
};
class Xhr {
  static instance: Xhr;
  upload = {
    onprogress: null as
      | ((event: {
          lengthComputable: boolean;
          loaded: number;
          total: number;
        }) => void)
      | null,
  };
  onload: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onabort: (() => void) | null = null;
  status = 201;
  responseText = '';
  open = vi.fn();
  setRequestHeader = vi.fn();
  send = vi.fn();
  abort = vi.fn(() => this.onabort?.());
  constructor() {
    Xhr.instance = this;
  }
}
beforeEach(() => vi.stubGlobal('XMLHttpRequest', Xhr));
afterEach(() => vi.unstubAllGlobals());

it('uses encoded relative multipart route and real byte progress, resolving only after valid server success', async () => {
  const progress = vi.fn();
  const pending = uploadAttachmentWithProgress(
    'agent/a',
    'chat:1',
    new File(['png'], 'photo.png', { type: 'image/png' }),
    { onProgress: progress },
  );
  const xhr = Xhr.instance;
  expect(xhr.open).toHaveBeenCalledWith(
    'POST',
    '/api/agents/agent%2Fa/sessions/chat%3A1/attachments',
  );
  expect(xhr.setRequestHeader).toHaveBeenCalledWith(
    'accept',
    'application/json',
  );
  expect(
    xhr.setRequestHeader.mock.calls.some(
      ([name]) => name.toLowerCase() === 'content-type',
    ),
  ).toBe(false);
  expect(Array.from((xhr.send.mock.calls[0][0] as FormData).keys())).toEqual([
    'file',
  ]);
  xhr.upload.onprogress?.({ lengthComputable: true, loaded: 2, total: 4 });
  expect(progress).toHaveBeenLastCalledWith(50);
  xhr.upload.onprogress?.({ lengthComputable: false, loaded: 2, total: 0 });
  expect(progress).toHaveBeenLastCalledWith(null);
  xhr.responseText = JSON.stringify({
    attachment: { ...metadata, data: 'must not escape' },
  });
  xhr.onload?.();
  await expect(pending).resolves.toEqual(metadata);
  expect(xhr.onload).toBeNull();
  expect(xhr.upload.onprogress).toBeNull();
});

it('classifies bounded WAV by extension only and keeps audio as an ordinary file reference', () => {
  const audio = {
    ...metadata,
    name: 'voice.WAV',
    mime: 'audio/wav',
    kind: 'audio' as const,
    size: 4 * 1024 * 1024,
  };
  const file = new File(['not browser-decoded'], 'voice.WAV', {
    type: 'application/octet-stream',
  });
  Object.defineProperty(file, 'size', {
    value: audio.size,
    configurable: true,
  });
  expect(classifyAttachmentFile(file)).toEqual({ kind: 'audio' });
  expect(attachmentMetadata(audio)).toEqual(audio);
  expect(messageAttachment(audio)).toMatchObject({
    type: 'file',
    kind: 'audio',
  });
  Object.defineProperty(file, 'size', { value: audio.size + 1 });
  expect(classifyAttachmentFile(file)).toEqual({
    error: ATTACHMENT_ERRORS.tooLarge,
  });
  expect(attachmentMetadata({ ...audio, size: audio.size + 1 })).toBeNull();
  expect(attachmentMetadata({ ...audio, mime: 'audio/mpeg' })).toBeNull();
  expect(
    classifyAttachmentFile(new File(['x'], 'voice.mp3', { type: 'audio/wav' })),
  ).toEqual({ error: ATTACHMENT_ERRORS.unsupported });
  expect(classifyAttachmentFile(new File([], 'empty.wav'))).toEqual({
    error: ATTACHMENT_ERRORS.empty,
  });
});

it('maps HTTP failure before accepting metadata', async () => {
  const pending = uploadAttachmentWithProgress(
    'a',
    's',
    new File(['x'], 'x.txt'),
  );
  Xhr.instance.status = 403;
  Xhr.instance.responseText = JSON.stringify({ error: 'Denied' });
  Xhr.instance.onload?.();
  await expect(pending).rejects.toBeInstanceOf(DaemonHttpError);
});

it('rejects malformed metadata and handles transport abort and network errors', async () => {
  const invalid = uploadAttachmentWithProgress(
    'a',
    's',
    new File(['x'], 'x.txt'),
  );
  Xhr.instance.responseText = '{}';
  Xhr.instance.onload?.();
  await expect(invalid).rejects.toThrow(ATTACHMENT_ERRORS.invalidResponse);
  const controller = new AbortController();
  const aborted = uploadAttachmentWithProgress(
    'a',
    's',
    new File(['x'], 'x.txt'),
    { signal: controller.signal },
  );
  controller.abort();
  await expect(aborted).rejects.toMatchObject({ name: 'AbortError' });
  expect(Xhr.instance.abort).toHaveBeenCalledOnce();
  const failed = uploadAttachmentWithProgress(
    'a',
    's',
    new File(['x'], 'x.txt'),
  );
  Xhr.instance.onerror?.();
  await expect(failed).rejects.toThrow(ATTACHMENT_ERRORS.upload);
});

it('validates public metadata and obvious file limits without accepting bytes/URLs', () => {
  expect(attachmentMetadata(metadata)).toEqual(metadata);
  for (const patch of [
    { id: '' },
    { size: Infinity },
    { kind: 'script' },
    { mime: 'x'.repeat(129) },
    { path: '../secret' },
  ])
    expect(attachmentMetadata({ ...metadata, ...patch })).toBeNull();
  expect(classifyAttachmentFile(new File(['x'], 'brief.md'))).toEqual({
    kind: 'text',
  });
  expect(classifyAttachmentFile(new File(['x'], 'file.pdf'))).toEqual({
    kind: 'document',
  });
  expect(classifyAttachmentFile(new File([], 'empty.txt'))).toEqual({
    error: ATTACHMENT_ERRORS.empty,
  });
  expect(
    classifyAttachmentFile(
      new File(['x'], 'file.svg', { type: 'image/svg+xml' }),
    ),
  ).toEqual({ error: ATTACHMENT_ERRORS.unsupported });
  const huge = new File(['x'], 'file.txt');
  Object.defineProperty(huge, 'size', { value: 1024 * 1024 + 1 });
  expect(classifyAttachmentFile(huge)).toEqual({
    error: ATTACHMENT_ERRORS.tooLarge,
  });
});

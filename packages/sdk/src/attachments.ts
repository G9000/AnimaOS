import type { DaemonClient } from './client.js';

/** Durable session upload metadata; file bytes are fetched separately. */
export interface UploadedAttachment {
  id: string;
  name: string;
  mime: string;
  size: number;
  kind: 'image' | 'text' | 'document' | 'audio';
  path: string;
  createdAtMs: number;
}

export class AttachmentsClient {
  constructor(private readonly client: DaemonClient) {}

  async upload(
    agentId: string,
    sessionId: string,
    file: Blob,
    name: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<{ attachment: UploadedAttachment }> {
    const body = new FormData();
    body.append('file', file, name);
    return this.client.requestJson<{ attachment: UploadedAttachment }>(
      `/api/agents/${encodeURIComponent(agentId)}/sessions/${encodeURIComponent(sessionId)}/attachments`,
      { method: 'POST', body, signal: options.signal },
    );
  }

  async download(
    agentId: string,
    attachmentId: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<Blob> {
    return this.client.requestBlob(
      `/api/agents/${encodeURIComponent(agentId)}/attachments/${encodeURIComponent(attachmentId)}`,
      { signal: options.signal },
    );
  }
}

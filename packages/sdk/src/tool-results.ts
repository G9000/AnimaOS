import type { DaemonClient } from './client.js';

export interface ToolResultArtifact {
  id: string;
  agentId: string;
  runId: string;
  toolCallId: string;
  toolName: string;
  bytes: number;
  sha256: string;
  captureComplete: boolean;
  createdAtMs: number;
}
export interface ToolResultPage {
  artifact: ToolResultArtifact;
  /** Exact UTF-8 JSON payload bytes decoded as text; offsets are byte offsets. */
  text: string;
  offset: number;
  nextOffset: number | null;
}
export class ToolResultsClient {
  constructor(private readonly client: DaemonClient) {}
  read(
    agentId: string,
    id: string,
    options: { offset?: number; limit?: number; signal?: AbortSignal } = {},
  ): Promise<ToolResultPage> {
    const query = new URLSearchParams();
    if (options.offset !== undefined)
      query.set('offset', String(options.offset));
    if (options.limit !== undefined) query.set('limit', String(options.limit));
    const suffix = query.size ? `?${query}` : '';
    return this.client.requestJson<ToolResultPage>(
      `${artifactPath(agentId, id)}${suffix}`,
      { signal: options.signal },
    );
  }
  download(
    agentId: string,
    id: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<Blob> {
    return this.client.requestBlob(`${artifactPath(agentId, id)}/download`, {
      signal: options.signal,
    });
  }
}

function artifactPath(agentId: string, id: string): string {
  return `/api/agents/${encodeURIComponent(agentId)}/tool-results/${encodeURIComponent(id)}`;
}

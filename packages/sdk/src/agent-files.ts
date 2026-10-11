import type { DaemonClient } from './client.js';
import type { AgentFilesPreview } from './daemon-types.js';

/** Owner-only, content-free view of files available to a future run. */
export class AgentFilesClient {
  constructor(private readonly client: DaemonClient) {}

  preview(
    agentId: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<AgentFilesPreview> {
    return this.client.requestJson<AgentFilesPreview>(
      `/api/agents/${encodeURIComponent(agentId)}/agent-files`,
      { signal: options.signal },
    );
  }
}

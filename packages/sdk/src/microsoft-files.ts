import type { DaemonClient } from './client.js';

export type MicrosoftFileMode = 'oneDrive' | 'sharePoint';
export interface MicrosoftFileConnector {
  id: string;
  mode: MicrosoftFileMode;
  status: 'pairing' | 'active' | 'reauthRequired' | 'disconnected';
  generation: string;
  selectionRevision: number;
  accountLabel: string | null;
  approvedSiteUrl: string | null;
  selectedFiles: { key: string; name: string }[];
}
export interface MicrosoftFileStatus {
  configured: boolean;
  connectors: MicrosoftFileConnector[];
}
export interface MicrosoftFileChoice {
  key: string;
  name: string;
  isFolder: boolean;
  mime: string | null;
}
export interface MicrosoftFileChoices {
  files: MicrosoftFileChoice[];
  nextCursor: string | null;
}
type ReadOptions = { signal?: AbortSignal };

export class MicrosoftFilesClient {
  constructor(private readonly client: DaemonClient) {}

  status(agentId: string, options: ReadOptions = {}): Promise<MicrosoftFileStatus> {
    return this.client.requestJson(base(agentId), options);
  }
  connect(agentId: string, input: { mode: MicrosoftFileMode; approvedSiteUrl?: string | null }): Promise<{ connectorId: string; consentUrl: string; expiresAtMs: number }> {
    return this.client.requestJson(`${base(agentId)}/connect`, { method: 'POST', body: input });
  }
  files(agentId: string, connectorId: string, input: { folderKey?: string; cursor?: string } = {}, options: ReadOptions = {}): Promise<MicrosoftFileChoices> {
    const params = new URLSearchParams();
    if (input.folderKey) params.set('folderKey', input.folderKey);
    if (input.cursor) params.set('cursor', input.cursor);
    const query = params.toString();
    return this.client.requestJson(`${base(agentId)}/${encodeURIComponent(connectorId)}/files${query ? `?${query}` : ''}`, options);
  }
  async select(agentId: string, connectorId: string, input: { expectedGeneration: string; expectedSelectionRevision: number; fileKeys: string[] }): Promise<MicrosoftFileConnector> {
    const result = await this.client.requestJson<{ connector: MicrosoftFileConnector }>(`${base(agentId)}/${encodeURIComponent(connectorId)}/selection`, { method: 'PUT', body: input });
    return result.connector;
  }
  disconnect(agentId: string, connectorId: string, input: { expectedGeneration: string }): Promise<{ disconnected: boolean }> {
    return this.client.requestJson(`${base(agentId)}/${encodeURIComponent(connectorId)}`, { method: 'DELETE', body: input });
  }
}
function base(agentId: string): string {
  return `/api/agents/${encodeURIComponent(agentId)}/connectors/microsoft-files`;
}

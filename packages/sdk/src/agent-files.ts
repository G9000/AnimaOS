import type { DaemonClient } from './client.js';
import type { AgentFilesPreview } from './daemon-types.js';

/** The saved personality source read by newly started turns. */
export interface AgentPersonality {
  source: 'agent_config' | 'workspace_soul' | 'profile_soul';
  path: string | null;
  content: string;
  revision: string;
  /** Maximum Unicode code points, rather than UTF-16 code units. */
  maxChars: number;
}

export interface SaveAgentPersonality {
  content: string;
  revision: string;
}

/** Owner-only personality editing and instruction file previews. */
export class AgentFilesClient {
  constructor(private readonly client: DaemonClient) {}

  personality(
    agentId: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<AgentPersonality> {
    return this.client.requestJson<AgentPersonality>(
      `/api/agents/${encodeURIComponent(agentId)}/personality`,
      { signal: options.signal },
    );
  }

  savePersonality(
    agentId: string,
    input: SaveAgentPersonality,
  ): Promise<AgentPersonality> {
    return this.client.requestJson<AgentPersonality>(
      `/api/agents/${encodeURIComponent(agentId)}/personality`,
      { method: 'PUT', body: input },
    );
  }

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

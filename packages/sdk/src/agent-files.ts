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

/** Owner-selected, pinned context source; credentials stay in its MCP connection. */
export interface WorkspaceContextBinding {
  connectionId: string;
  tool: string;
  mode: 'companion' | 'shared';
  workspaceId: string;
  generation: string;
  subject: string;
}

export interface WorkspaceContextView {
  revision: string;
  binding: WorkspaceContextBinding | null;
  status: 'independent' | 'configured';
  connections: { id: string; label: string; state: string; tool: string }[];
}

export interface SaveWorkspaceContext {
  expectedRevision: string;
  binding: WorkspaceContextBinding | null;
}

/** Owner-only personality editing and instruction file previews. */
export class AgentFilesClient {
  constructor(private readonly client: DaemonClient) {}

  workspaceContext(
    agentId: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<WorkspaceContextView> {
    return this.client.requestJson<WorkspaceContextView>(
      `/api/agents/${encodeURIComponent(agentId)}/workspace-context`,
      { signal: options.signal },
    );
  }

  saveWorkspaceContext(
    agentId: string,
    input: SaveWorkspaceContext,
  ): Promise<WorkspaceContextView> {
    return this.client.requestJson<WorkspaceContextView>(
      `/api/agents/${encodeURIComponent(agentId)}/workspace-context`,
      { method: 'PUT', body: input },
    );
  }

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

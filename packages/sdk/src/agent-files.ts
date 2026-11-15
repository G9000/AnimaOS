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

/** Additive, agent-bound shared memory. Credentials stay in the reviewed MCP connection. */
export interface MemoryProviderBinding {
  version: 1;
  provider: 'mother';
  connectionId: string;
  workspaceId: string;
  generation: string;
  subject: string;
  publication: 'selected';
  /** Remote grant mode, independent of the personality choice. */
  accessMode: 'companion' | 'shared';
  /** Workspace personality requires a companion grant on this same workspace. */
  personaSource: 'local' | 'workspace';
}

export interface WorkspaceContextView {
  revision: string;
  binding: WorkspaceContextBinding | null;
  /** Absent on daemons predating additive providers. */
  providerBinding?: MemoryProviderBinding | null;
  status: 'independent' | 'configured';
  connections: { id: string; label: string; state: string; tool: string }[];
}

export interface SaveWorkspaceContext {
  expectedRevision: string;
  binding: WorkspaceContextBinding | null;
  /** Omitted preserves it; null disables it. Set with binding:null for atomic conversion. */
  providerBinding?: MemoryProviderBinding | null;
}

/** Owner-only personality editing and instruction file previews. */
export class AgentFilesClient {
  constructor(private readonly client: DaemonClient) {}

  memoryPublications(agentId: string, options: { signal?: AbortSignal } = {}): Promise<import('./memory-publications.js').MemoryPublicationsView> {
    return this.client.requestJson(`/api/agents/${encodeURIComponent(agentId)}/memory-publications`, { signal: options.signal });
  }

  memoryPublication(agentId: string, publicationId: string, options: { signal?: AbortSignal } = {}): Promise<import('./memory-publications.js').MemoryPublication> {
    return this.client.requestJson(`/api/agents/${encodeURIComponent(agentId)}/memory-publications/${encodeURIComponent(publicationId)}`, { signal: options.signal });
  }

  shareMemory(agentId: string, input: import('./memory-publications.js').ShareMemory): Promise<import('./memory-publications.js').MemoryPublication> {
    return this.client.requestJson(`/api/agents/${encodeURIComponent(agentId)}/memory-publications`, { method: 'POST', body: input });
  }

  controlMemoryPublication(agentId: string, publicationId: string, input: import('./memory-publications.js').ControlMemoryPublication): Promise<import('./memory-publications.js').MemoryPublication> {
    return this.client.requestJson(`/api/agents/${encodeURIComponent(agentId)}/memory-publications/${encodeURIComponent(publicationId)}/control`, { method: 'POST', body: input });
  }

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

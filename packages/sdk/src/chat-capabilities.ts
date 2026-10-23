import type { DaemonClient } from './client.js';

export interface ChatToolCapability {
  name: string;
  description: string;
  origin: 'native' | 'mcp';
  risk: 'read' | 'write' | 'exec' | 'network' | 'delegate';
  /** Policy for an unqualified call; argument rules and safeguards still apply. */
  approval: 'allow' | 'ask' | 'deny';
  argumentDependent: boolean;
  available: boolean;
  reason: string | null;
}

export interface ChatCapabilities {
  agentId: string;
  sessionId: string;
  basis: 'active_turn' | 'next_owner_turn';
  runId: string | null;
  preparing: boolean;
  tools: ChatToolCapability[];
  connections: { id: string; label: string; state: string }[];
  notice: string;
}

export class ChatCapabilitiesClient {
  constructor(private readonly client: DaemonClient) {}
  get(agentId: string, sessionId: string, options: { signal?: AbortSignal } = {}): Promise<ChatCapabilities> {
    return this.client.requestJson(`/api/agents/${encodeURIComponent(agentId)}/sessions/${encodeURIComponent(sessionId)}/capabilities`, { signal: options.signal });
  }
}

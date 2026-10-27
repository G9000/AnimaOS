import type { DaemonClient } from './client.js';

export type McpConnectionState =
  | 'disconnected'
  | 'connecting'
  | 'needs_review'
  | 'ready'
  | 'degraded'
  | 'stopping';

export interface McpToolDescriptor {
  remoteName: string;
  description: string;
  inputSchema: Record<string, unknown>;
  outputSchema: Record<string, unknown> | null;
  descriptorDigest: string;
}

export interface ReviewedMcpTool extends McpToolDescriptor {
  alias: string;
}

export interface McpStaticError {
  code: string;
  message: string;
}

/** Immutable local-process launch. No shell, environment or installation. */
export interface McpStdioSpec {
  executable: string;
  args: string[];
}

/** Owner-visible metadata and runtime status for one saved MCP connection. */
export interface McpConnection {
  id: string;
  agentId: string;
  label: string;
  transport: 'streamableHttp' | 'stdio';
  stdio: McpStdioSpec | null;
  /** Empty for a stdio connection. */
  endpoint: string;
  enabled: boolean;
  version: number;
  /** Saved credential reference only; not vault availability or remote validity. */
  hasBearerCredential: boolean;
  /** Saved account reference; validity is checked separately through OAuth status. */
  hasOAuthCredential?: boolean;
  reviewedTools: ReviewedMcpTool[];
  /** Current agent policy; older daemons require manual review and grants. */
  toolAccess?: 'automatic' | 'review' | 'denied';
  state: McpConnectionState;
  wireGeneration: number;
  discoveryId: string | null;
  candidates: McpToolDescriptor[];
  physicalCallsActive: boolean;
  error: McpStaticError | null;
}

export type McpCreateInput =
  | { label: string; endpoint: string; stdio?: never }
  | { label: string; stdio: McpStdioSpec; endpoint?: never };

export interface McpUpdateInput {
  expectedVersion: number;
  label?: string;
  /** HTTP only. Local-process launch configuration cannot be edited. */
  endpoint?: string;
  enabled?: boolean;
}

export interface McpVersionInput {
  expectedVersion: number;
}

export interface McpBearerCredentialInput extends McpVersionInput {
  bearerToken: string;
}

export interface McpOAuthStartInput extends McpVersionInput {
  clientId?: string;
}
export interface McpOAuthStart {
  authorizationUrl: string;
  attemptId: string;
  expiresAt: number;
}
export interface McpOAuthStatus {
  state: 'signed_out' | 'pending' | 'connected' | 'needs_sign_in' | 'failed';
  attemptId?: string | null;
  expiresAt?: number | null;
  error?: McpStaticError | null;
}
export interface McpOAuthCancelInput extends McpVersionInput {
  attemptId: string;
}

export interface McpReviewInput extends McpVersionInput {
  discoveryId: string;
  descriptorDigest: string;
}

export interface McpGrantInput extends McpVersionInput {
  descriptorDigest: string;
  granted: boolean;
}

export interface McpGrantOutcome {
  connection: McpConnection;
  granted: boolean;
  toolNames: string[];
}

export interface McpReadOptions {
  signal?: AbortSignal;
}

/** Typed access to the owner's saved, reviewed, and granted MCP connections. */
export class McpClient {
  constructor(private readonly client: DaemonClient) {}

  async list(
    agentId: string,
    options: McpReadOptions = {},
  ): Promise<McpConnection[]> {
    const response = await this.client.requestJson<{
      connections: McpConnection[];
    }>(agentPath(agentId), { signal: options.signal });
    return response.connections;
  }

  async get(
    agentId: string,
    connectionId: string,
    options: McpReadOptions = {},
  ): Promise<McpConnection> {
    const response = await this.client.requestJson<{
      connection: McpConnection;
    }>(connectionPath(agentId, connectionId), { signal: options.signal });
    return response.connection;
  }

  async create(agentId: string, input: McpCreateInput): Promise<McpConnection> {
    const response = await this.client.requestJson<{
      connection: McpConnection;
    }>(agentPath(agentId), { method: 'POST', body: input });
    return response.connection;
  }

  async update(
    agentId: string,
    connectionId: string,
    input: McpUpdateInput,
  ): Promise<McpConnection> {
    const response = await this.client.requestJson<{
      connection: McpConnection;
    }>(connectionPath(agentId, connectionId), {
      method: 'PATCH',
      body: input,
    });
    return response.connection;
  }

  async connect(
    agentId: string,
    connectionId: string,
    input: McpVersionInput,
  ): Promise<McpConnection> {
    return this.connectionAction(agentId, connectionId, 'connect', input);
  }

  async discover(
    agentId: string,
    connectionId: string,
    input: McpVersionInput,
  ): Promise<McpConnection> {
    return this.connectionAction(agentId, connectionId, 'discover', input);
  }

  async review(
    agentId: string,
    connectionId: string,
    input: McpReviewInput,
  ): Promise<McpConnection> {
    return this.connectionAction(agentId, connectionId, 'review', input);
  }

  async disconnect(
    agentId: string,
    connectionId: string,
    input: McpVersionInput,
  ): Promise<McpConnection> {
    return this.connectionAction(agentId, connectionId, 'disconnect', input);
  }

  async remove(
    agentId: string,
    connectionId: string,
    input: McpVersionInput,
  ): Promise<void> {
    await this.client.requestJson<unknown>(
      connectionPath(agentId, connectionId),
      {
        method: 'DELETE',
        body: input,
      },
    );
  }

  async saveBearerCredential(
    agentId: string,
    connectionId: string,
    input: McpBearerCredentialInput,
  ): Promise<McpConnection> {
    const response = await this.client.requestJson<{
      connection: McpConnection;
    }>(`${connectionPath(agentId, connectionId)}/credential`, {
      method: 'PUT',
      body: input,
    });
    return response.connection;
  }

  async clearBearerCredential(
    agentId: string,
    connectionId: string,
    input: McpVersionInput,
  ): Promise<McpConnection> {
    const response = await this.client.requestJson<{
      connection: McpConnection;
    }>(`${connectionPath(agentId, connectionId)}/credential`, {
      method: 'DELETE',
      body: input,
    });
    return response.connection;
  }

  async setGrant(
    agentId: string,
    connectionId: string,
    alias: string,
    input: McpGrantInput,
  ): Promise<McpGrantOutcome> {
    return this.client.requestJson<McpGrantOutcome>(
      `${connectionPath(agentId, connectionId)}/grants/${encodeURIComponent(alias)}`,
      { method: 'PATCH', body: input },
    );
  }

  async startOAuth(
    agentId: string,
    connectionId: string,
    input: McpOAuthStartInput,
  ): Promise<McpOAuthStart> {
    return this.client.requestJson<McpOAuthStart>(
      `${connectionPath(agentId, connectionId)}/oauth/start`,
      {
        method: 'POST',
        body: input,
      },
    );
  }

  async getOAuthStatus(
    agentId: string,
    connectionId: string,
    options: McpReadOptions = {},
  ): Promise<McpOAuthStatus> {
    return this.client.requestJson<McpOAuthStatus>(
      `${connectionPath(agentId, connectionId)}/oauth/status`,
      options,
    );
  }

  async cancelOAuth(
    agentId: string,
    connectionId: string,
    input: McpOAuthCancelInput,
  ): Promise<McpConnection> {
    const response = await this.client.requestJson<{
      connection: McpConnection;
    }>(`${connectionPath(agentId, connectionId)}/oauth/cancel`, {
      method: 'POST',
      body: input,
    });
    return response.connection;
  }

  async signOutOAuth(
    agentId: string,
    connectionId: string,
    input: McpVersionInput,
  ): Promise<McpConnection> {
    const response = await this.client.requestJson<{
      connection: McpConnection;
    }>(`${connectionPath(agentId, connectionId)}/oauth`, {
      method: 'DELETE',
      body: input,
    });
    return response.connection;
  }

  private async connectionAction(
    agentId: string,
    connectionId: string,
    action: 'connect' | 'discover' | 'review' | 'disconnect',
    input: McpVersionInput | McpReviewInput,
  ): Promise<McpConnection> {
    const response = await this.client.requestJson<{
      connection: McpConnection;
    }>(`${connectionPath(agentId, connectionId)}/${action}`, {
      method: 'POST',
      body: input,
    });
    return response.connection;
  }
}

function agentPath(agentId: string): string {
  return `/api/agents/${encodeURIComponent(agentId)}/mcp-connections`;
}

function connectionPath(agentId: string, connectionId: string): string {
  return `${agentPath(agentId)}/${encodeURIComponent(connectionId)}`;
}

import type { DaemonClient } from './client.js';

export interface BrowserSelection {
  policyId: string;
  selectedInputIds: string[];
}

export interface BrowserLimits {
  actions: number;
  requests: number;
  scopeMs: number;
  frameBytes: number;
  wireBytes: number;
  observationNodes: number;
  observationTextBytes: number;
  inputBytes: number;
  outputBytes: number;
  screenshotBytes: number;
}

/** Operator-installed destinations; reading them opens no browser. */
export interface BrowserPolicy {
  id: string;
  origin: string;
  routes: { id: string; method: 'GET' | 'POST'; path: string }[];
  policyDigest: string;
  installationId: string;
  installationDigest: string;
  limits: BrowserLimits;
  workerAvailable: boolean;
}

export interface BrowserDocument {
  documentId: string;
  documentGeneration: number;
  observationDigest: string;
}

export interface BrowserArtifact {
  id: string;
  kind: 'screenshot' | 'csv';
  bytes: number;
  sha256: string;
}

/** Runtime-only scope. A grant, enabled flag or close acknowledgement does not prove a physical lease is released. */
export interface BrowserScope {
  id: string;
  agentId: string;
  runId: string;
  sessionId: string;
  version: number;
  enabled: boolean;
  state:
    | 'disabled'
    | 'scoped'
    | 'active'
    | 'stopping'
    | 'closed'
    | 'unavailable';
  policyId: string;
  origin: string;
  policyDigest: string;
  installationId: string;
  installationDigest: string;
  selectedInputIds: string[];
  expiresAtMs: number;
  actionsUsed: number;
  requestsUsed: number;
  wireBytesUsed: number;
  physicalLeaseHeld: boolean;
  document: BrowserDocument | null;
  artifacts: BrowserArtifact[];
  toolNames: string[];
  error: string | null;
}

export interface SaveBrowserScopeInput extends BrowserSelection {
  enabled: boolean;
  expectedVersion?: number;
}

export interface StopBrowserScopeInput {
  expectedVersion: number;
}

export interface BrowserReadOptions {
  signal?: AbortSignal;
}

/** Owner transport only: never grants, starts a worker, renews a scope or retries. */
export class BrowserScopesClient {
  constructor(private readonly client: DaemonClient) {}

  async get(
    agentId: string,
    runId: string,
    options: BrowserReadOptions = {},
  ): Promise<BrowserScope | null> {
    const response = await this.client.requestJson<{
      scope: BrowserScope | null;
    }>(scopePath(agentId, runId), { signal: options.signal });
    return response.scope;
  }

  async list(
    agentId: string,
    options: BrowserReadOptions = {},
  ): Promise<BrowserScope[]> {
    const response = await this.client.requestJson<{ scopes: BrowserScope[] }>(
      `${agentPath(agentId)}/browser-scopes`,
      { signal: options.signal },
    );
    return response.scopes;
  }

  async policies(
    agentId: string,
    options: BrowserReadOptions = {},
  ): Promise<BrowserPolicy[]> {
    const response = await this.client.requestJson<{
      policies: BrowserPolicy[];
    }>(`${agentPath(agentId)}/browser-policies`, { signal: options.signal });
    return response.policies;
  }

  async save(
    agentId: string,
    runId: string,
    input: SaveBrowserScopeInput,
  ): Promise<BrowserScope> {
    const response = await this.client.requestJson<{ scope: BrowserScope }>(
      scopePath(agentId, runId),
      {
        method: 'PUT',
        body: {
          policyId: input.policyId,
          selectedInputIds: input.selectedInputIds,
          enabled: input.enabled,
          ...(input.expectedVersion === undefined
            ? {}
            : { expectedVersion: input.expectedVersion }),
        },
      },
    );
    return response.scope;
  }

  async stop(
    agentId: string,
    runId: string,
    input: StopBrowserScopeInput,
  ): Promise<BrowserScope> {
    const response = await this.client.requestJson<{ scope: BrowserScope }>(
      `${scopePath(agentId, runId)}/stop`,
      { method: 'POST', body: { expectedVersion: input.expectedVersion } },
    );
    return response.scope;
  }
}

function agentPath(agentId: string): string {
  return `/api/agents/${encodeURIComponent(agentId)}`;
}

function scopePath(agentId: string, runId: string): string {
  return `${agentPath(agentId)}/runs/${encodeURIComponent(runId)}/browser-scope`;
}

import type { DaemonClient } from './client.js';

export interface BrowserSelection {
  policyId: string;
  profileId?: string;
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

export interface BrowserSetup {
  available: boolean;
  reason: string | null;
  setupCommand: string;
}

export interface CreateBrowserDestinationInput {
  startUrl: string;
  allowedOrigins: string[];
  authenticated: boolean;
}

export interface BrowserProfile {
  id: string;
  agentId: string;
  policyId: string;
  origin: string;
  revision: number;
  digest: string;
  ready: boolean;
  revoked: boolean;
  loginActive: boolean;
}

export interface BrowserProfileMutationInput {
  expectedRevision: number;
}

export interface StartBrowserProfileLoginInput extends BrowserProfileMutationInput {
  routeId: string;
}

/** Reviewed destinations; reading them opens no browser. */
export interface BrowserPolicy {
  id: string;
  origin: string;
  routes: { id: string; method: 'GET' | 'POST'; path: string }[];
  policyDigest: string;
  installationId: string;
  installationDigest: string;
  limits: BrowserLimits;
  workerAvailable: boolean;
  authenticated: boolean;
  interactive: boolean;
  allowedOrigins: string[];
  version: number;
}

export interface BrowserDocument {
  documentId: string;
  documentGeneration: number;
  observationDigest: string;
}

export interface BrowserArtifact {
  id: string;
  kind: 'screenshot' | 'csv' | 'text' | 'json' | 'pdf' | 'png' | 'jpeg';
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
    | 'ownerControl'
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
  ownerControl: boolean;
  profile: BrowserProfile | null;
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

  setup(
    agentId: string,
    options: BrowserReadOptions = {},
  ): Promise<BrowserSetup> {
    return this.client.requestJson(
      `${agentPath(agentId)}/browser-setup`,
      options,
    );
  }

  async createDestination(
    agentId: string,
    input: CreateBrowserDestinationInput,
  ): Promise<BrowserPolicy> {
    const response = await this.client.requestJson<{ policy: BrowserPolicy }>(
      `${agentPath(agentId)}/browser-destinations`,
      { method: 'POST', body: input },
    );
    return response.policy;
  }

  revokeDestination(
    agentId: string,
    policyId: string,
    input: { expectedVersion: number },
  ): Promise<{ revoked: boolean }> {
    return this.client.requestJson(
      `${agentPath(agentId)}/browser-destinations/${encodeURIComponent(policyId)}/revoke`,
      { method: 'POST', body: input },
    );
  }

  async profiles(
    agentId: string,
    options: BrowserReadOptions = {},
  ): Promise<BrowserProfile[]> {
    const response = await this.client.requestJson<{
      profiles: BrowserProfile[];
    }>(`${agentPath(agentId)}/browser-profiles`, options);
    return response.profiles;
  }

  async createProfile(
    agentId: string,
    input: { policyId: string },
  ): Promise<BrowserProfile> {
    const response = await this.client.requestJson<{ profile: BrowserProfile }>(
      `${agentPath(agentId)}/browser-profiles`,
      { method: 'POST', body: input },
    );
    return response.profile;
  }

  async startProfileLogin(
    agentId: string,
    profileId: string,
    input: StartBrowserProfileLoginInput,
  ): Promise<BrowserProfile> {
    return this.profileMutation(agentId, profileId, 'sign-in/start', input);
  }

  async finishProfileLogin(
    agentId: string,
    profileId: string,
    input: BrowserProfileMutationInput,
  ): Promise<BrowserProfile> {
    return this.profileMutation(agentId, profileId, 'sign-in/finish', input);
  }

  async revokeProfile(
    agentId: string,
    profileId: string,
    input: BrowserProfileMutationInput,
  ): Promise<BrowserProfile> {
    return this.profileMutation(agentId, profileId, 'revoke', input);
  }

  private async profileMutation(
    agentId: string,
    profileId: string,
    action: string,
    input: BrowserProfileMutationInput,
  ): Promise<BrowserProfile> {
    const response = await this.client.requestJson<{ profile: BrowserProfile }>(
      `${agentPath(agentId)}/browser-profiles/${encodeURIComponent(profileId)}/${action}`,
      { method: 'POST', body: input },
    );
    return response.profile;
  }

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
          ...(input.profileId === undefined
            ? {}
            : { profileId: input.profileId }),
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

  async takeover(
    agentId: string,
    runId: string,
    input: StopBrowserScopeInput,
  ): Promise<BrowserScope> {
    return this.control(agentId, runId, 'takeover', input);
  }

  async resume(
    agentId: string,
    runId: string,
    input: StopBrowserScopeInput,
  ): Promise<BrowserScope> {
    return this.control(agentId, runId, 'resume', input);
  }

  private async control(
    agentId: string,
    runId: string,
    action: 'takeover' | 'resume',
    input: StopBrowserScopeInput,
  ): Promise<BrowserScope> {
    const response = await this.client.requestJson<{ scope: BrowserScope }>(
      `${scopePath(agentId, runId)}/${action}`,
      { method: 'POST', body: input },
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

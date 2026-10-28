import type { DaemonClient } from './client.js';

export type ConnectedProvider =
  'gdrive' | 'gmail' | 'outlook' | 'email' | 'gcalendar' | 'onedrive' | 'sharepoint';
export type ConnectedKind = 'file' | 'mail' | 'event';
export interface ConnectedSearchSettings {
  historyDepth: 'recent' | 'days30' | 'days90' | 'days365';
  retainedItemLimit: 100 | 500 | 2000;
  attachmentText: 'metadataOnly' | 'supportedText';
  historyPaused: boolean;
}
export interface ConnectedHistoryCoverage {
  state: 'notRequested' | 'pending' | 'running' | 'paused' | 'complete' | 'partial' | 'unsupported' | 'error';
  requestedFromMs?: number | null;
  requestedBeforeMs?: number | null;
  reason?: string | null;
  enumeratedCount?: number;
  retainedCount?: number;
  oldestRetainedAt?: string | null;
  lastAttemptAtMs?: number | null;
  lastSuccessAtMs?: number | null;
  verifiedAtMs?: number | null;
  retryAtMs?: number | null;
}
export interface ConnectedSourceCapabilities {
  history: boolean;
  attachmentFormats: string[];
  workflowEventKinds: AccountEventKind[];
}
export interface ConnectedSource {
  connectorId: string;
  provider: ConnectedProvider;
  kind: ConnectedKind;
  accountLabel: string | null;
  generation: string;
  available: boolean;
  enabled: boolean;
  status:
    | 'disabled'
    | 'ready'
    | 'syncing'
    | 'stale'
    | 'reauthRequired'
    | 'unavailable';
  scope: string;
  itemLimit: number;
  itemCount: number;
  lastAttemptAtMs: number | null;
  lastSuccessAtMs: number | null;
  error: string | null;
  settingsRevision?: number;
  settings?: ConnectedSearchSettings;
  historyCoverage?: ConnectedHistoryCoverage;
  capabilities?: ConnectedSourceCapabilities;
  attachmentCoverage?: { indexed: number; partial: number; omitted: number; failed: number; pending: number; error: string | null };
}
export interface ConnectedSources {
  sources: ConnectedSource[];
  pollIntervalMs: number;
}
export interface ConnectedSearchHit {
  citationId: string;
  connectorId: string;
  provider: ConnectedProvider;
  kind: ConnectedKind;
  title: string;
  snippet: string;
  sourceUrl: string | null;
  citationPath: string;
  indexedAtMs: number;
  sourceModifiedAt: string | null;
  truncated: boolean;
  contentRole?: 'body' | 'file' | 'event' | 'attachment' | 'documentText' | null;
  parentCitationId?: string | null;
  extractionStatus?: string | null;
  omissions?: string[];
  locators?: string[];
}
export interface ConnectedSearchItem extends Omit<
  ConnectedSearchHit,
  'snippet'
> {
  text: string;
  sender: string | null;
  eventStart: string | null;
  eventEnd: string | null;
}
export interface ConnectedSearchQuery {
  query: string;
  connectorIds?: string[];
  kinds?: ConnectedKind[];
  limit?: number;
}
export interface ConnectedSearchResult {
  hits: ConnectedSearchHit[];
  sources: ConnectedSource[];
  omittedSourceCount?: number;
}
type ReadOptions = { signal?: AbortSignal };

export class ConnectedSearchClient {
  constructor(private readonly client: DaemonClient) {}
  sources(
    agentId: string,
    options: ReadOptions = {},
  ): Promise<ConnectedSources> {
    return this.client.requestJson(`${searchPath(agentId)}/sources`, options);
  }
  async setSource(
    agentId: string,
    connectorId: string,
    input: { enabled: boolean; expectedGeneration: string; expectedSettingsRevision?: number; settings?: ConnectedSearchSettings },
  ): Promise<ConnectedSource> {
    const result = await this.client.requestJson<{ source: ConnectedSource }>(
      `${searchPath(agentId)}/sources/${encodeURIComponent(connectorId)}`,
      { method: 'PUT', body: input },
    );
    return result.source;
  }
  refresh(
    agentId: string,
    input: { connectorId?: string; work?: 'live' | 'history' | 'all' } = {},
  ): Promise<{ sources: ConnectedSource[] }> {
    return this.client.requestJson(`${searchPath(agentId)}/refresh`, {
      method: 'POST',
      body: input,
    });
  }
  query(
    agentId: string,
    input: ConnectedSearchQuery,
    options: ReadOptions = {},
  ): Promise<ConnectedSearchResult> {
    return this.client.requestJson(`${searchPath(agentId)}/query`, {
      method: 'POST',
      body: input,
      signal: options.signal,
    });
  }
  async item(
    agentId: string,
    citationId: string,
    options: ReadOptions = {},
  ): Promise<ConnectedSearchItem> {
    const result = await this.client.requestJson<{ item: ConnectedSearchItem }>(
      `${searchPath(agentId)}/items/${encodeURIComponent(citationId)}`,
      options,
    );
    return result.item;
  }
}
function searchPath(agentId: string): string {
  return `/api/agents/${encodeURIComponent(agentId)}/connected-search`;
}

export type AccountEventKind =
  'mailObserved' | 'fileChanged' | 'calendarChanged';
export type AccountEventField =
  | 'eventId'
  | 'externalId'
  | 'title'
  | 'text'
  | 'sender'
  | 'sourceUrl'
  | 'collectionId'
  | 'observedAt';
export type AccountInputBinding =
  | { type: 'literal'; value: string }
  | { type: 'event'; field: AccountEventField };
export interface AccountWorkflowFilters {
  senderContains?: string | null;
  titleContains?: string | null;
  collectionId?: string | null;
}
export interface AccountWorkflowInput {
  /** Keep the same key and payload when retrying an uncertain creation. */
  requestKey?: string;
  name: string;
  connectorId: string;
  eventKind: AccountEventKind;
  templateSlug: string;
  templateRevision: number;
  filters: AccountWorkflowFilters;
  inputs: Record<string, AccountInputBinding>;
  enabled: boolean;
}
export interface AccountWorkflowRule extends Omit<
  AccountWorkflowInput,
  'requestKey'
> {
  id: string;
  agentId: string;
  revision: number;
  provider: ConnectedProvider;
  connectionGeneration: string;
  createdAtMs: number;
  updatedAtMs: number;
  baselineAtMs: number | null;
  deleted: boolean;
  lastOutcome: {
    status: 'started' | 'paused' | 'waiting';
    message: string;
    atMs: number;
  } | null;
}
export interface AccountWorkflowEvent {
  id: string;
  ruleId: string;
  ruleRevision: number;
  agentId: string;
  connectorId: string;
  connectionGeneration: string;
  eventId: string;
  eventKind: AccountEventKind;
  workflowId: string;
  observedAtMs: number;
  createdAtMs: number;
}
export class AccountWorkflowsClient {
  constructor(private readonly client: DaemonClient) {}
  async list(
    agentId: string,
    options: ReadOptions = {},
  ): Promise<AccountWorkflowRule[]> {
    return (
      await this.client.requestJson<{ rules: AccountWorkflowRule[] }>(
        rulesPath(agentId),
        options,
      )
    ).rules;
  }
  create(
    agentId: string,
    input: AccountWorkflowInput,
  ): Promise<AccountWorkflowRule> {
    return this.client.requestJson(rulesPath(agentId), {
      method: 'POST',
      body: input,
    });
  }
  update(
    agentId: string,
    ruleId: string,
    input: Partial<Omit<AccountWorkflowInput, 'requestKey'>> & {
      revision: number;
    },
  ): Promise<AccountWorkflowRule> {
    return this.client.requestJson(rulesPath(agentId, ruleId), {
      method: 'PATCH',
      body: input,
    });
  }
  delete(
    agentId: string,
    ruleId: string,
    input: { revision: number },
  ): Promise<AccountWorkflowRule> {
    return this.client.requestJson(rulesPath(agentId, ruleId), {
      method: 'DELETE',
      body: input,
    });
  }
  async events(
    agentId: string,
    ruleId: string,
    options: ReadOptions = {},
  ): Promise<AccountWorkflowEvent[]> {
    return (
      await this.client.requestJson<{ events: AccountWorkflowEvent[] }>(
        `${rulesPath(agentId, ruleId)}/events`,
        options,
      )
    ).events;
  }
}
function rulesPath(agentId: string, ruleId?: string): string {
  return `/api/agents/${encodeURIComponent(agentId)}/account-workflow-rules${ruleId === undefined ? '' : `/${encodeURIComponent(ruleId)}`}`;
}

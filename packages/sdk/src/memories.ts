import type {
  AgentRelationship,
  AgentRelationshipOptions,
  Memory,
  MemorySearchOptions,
  MemorySearchResult,
  MemoryScope,
  MemoryType,
  NewAgentRelationshipInput,
  RelationshipEndpointKind,
} from '@animaOS-SWARM/memory';

import type { DaemonClient } from './client.js';

export interface CreateMemoryInput {
  agentId: string;
  agentName: string;
  type: MemoryType;
  content: string;
  importance: number;
  tags?: string[] | null;
  scope?: MemoryScope;
  roomId?: string;
  worldId?: string;
  sessionId?: string;
}

export interface RecentMemoriesOptions {
  agentId?: string;
  agentName?: string;
  scope?: MemoryScope;
  roomId?: string;
  worldId?: string;
  sessionId?: string;
  limit?: number;
}

export type CreateAgentRelationshipInput = NewAgentRelationshipInput;

export interface MemoryEntity {
  kind: RelationshipEndpointKind;
  id: string;
  name: string;
  aliases: string[];
  summary?: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface CreateMemoryEntityInput {
  kind: RelationshipEndpointKind;
  id: string;
  name: string;
  aliases?: string[];
  summary?: string;
}

export interface MemoryEntityOptions {
  entityId?: string;
  kind?: RelationshipEndpointKind;
  name?: string;
  alias?: string;
  limit?: number;
}

export type MemoryEvaluationDecision = 'store' | 'merge' | 'ignore';

export interface MemoryEvaluation {
  decision: MemoryEvaluationDecision;
  reason: string;
  score: number;
  suggestedImportance: number;
  duplicateMemoryId?: string | null;
}

export interface EvaluatedMemoryInput extends CreateMemoryInput {
  minContentChars?: number;
  minImportance?: number;
}

export interface MemoryEvaluationOutcome {
  evaluation: MemoryEvaluation;
  memory?: Memory | null;
}

export interface MemoryRecallOptions extends MemorySearchOptions {
  entityId?: string;
  recallAgentId?: string;
  lexicalLimit?: number;
  recentLimit?: number;
  relationshipLimit?: number;
  temporalLimit?: number;
}

export interface MemoryRecallResult {
  memory: Memory;
  score: number;
  lexicalScore: number;
  vectorScore: number;
  relationshipScore: number;
  temporalScore: number;
  recencyScore: number;
  importanceScore: number;
}

/** The two core memory blocks: "About you" is `user`, "Agent notes" is `notes`. */
export type CoreMemoryBlockName = 'user' | 'notes';

/** Untrusted text: the companion may have written it, so show it as text. */
export interface CoreMemoryEntry {
  id: string;
  content: string;
  /** Scanner ids the text trips; the agent sees a flagged entry only as these. */
  flagged: string[];
}

export interface CoreMemoryBlock {
  /** Characters the entries use together, out of `limit`. */
  used: number;
  limit: number;
  /** In creation order. */
  entries: CoreMemoryEntry[];
}

export interface CoreMemory {
  user: CoreMemoryBlock;
  notes: CoreMemoryBlock;
}

/** The answer to an add, edit or delete: both blocks as they are now. */
export interface CoreMemoryEdit extends CoreMemory {
  /** The entry added, changed or removed. */
  entryId: string;
  /** False for an exact duplicate add, which changes nothing. */
  changed: boolean;
  /** Scanner ids on the new text; it was saved anyway. */
  warnings: string[];
}

export type MemoryExportFormat = 'json' | 'md';

export interface MemoryExportOptions {
  /** `json` (default) or `md`. */
  format?: MemoryExportFormat;
  /** One agent; every agent when absent. */
  agentId?: string;
}

export interface MemoryEvidenceTrace {
  memory: Memory;
  relationships: AgentRelationship[];
  entities: MemoryEntity[];
}

export interface MemoryImportanceAdjustment {
  memoryId: string;
  previousImportance: number;
  newImportance: number;
}

export interface MemoryRetentionInput {
  maxAgeMillis?: number;
  minImportance?: number;
  maxMemories?: number;
  decayHalfLifeMillis?: number;
}

export interface MemoryRetentionReport {
  decayedMemories: MemoryImportanceAdjustment[];
  removedMemoryIds: string[];
  removedRelationshipIds: string[];
}

/** Where the search model stands: it loads in the background after boot. */
export type MemoryEmbeddingLoadStatus =
  | 'downloading'
  | 'rebuilding'
  | 'ready'
  | 'fallback'
  | 'off';

export interface MemoryEmbeddingStatus {
  enabled: boolean;
  provider: string;
  model: string;
  dimension: number;
  vectorCount: number;
  persisted: boolean;
  storageFile?: string | null;
  status: MemoryEmbeddingLoadStatus;
  /** Memories with a vector from the active embedder, out of `total`. */
  done: number;
  total: number;
  /** Why search fell back to basic search; only with `fallback`. */
  reason?: string;
}

export interface MemoryEvalCheckResult {
  name: string;
  passed: boolean;
  detail: string;
}

export interface MemoryEvalCaseResult {
  name: string;
  checks: MemoryEvalCheckResult[];
}

export interface MemoryEvalReport {
  passed: boolean;
  totalChecks: number;
  passedChecks: number;
  failureMessages: string[];
  cases: MemoryEvalCaseResult[];
}

/** The external memory provider beside the local store; `none` by default. */
export interface ExternalMemoryProviderStatus {
  id: string;
  status: 'ready' | 'unavailable';
  /** Why it is unavailable; only with `unavailable`. */
  reason?: string;
}

export interface MemoryReadiness {
  passed: boolean;
  embeddings: MemoryEmbeddingStatus;
  /** Absent from daemons that predate the external provider plug-in point. */
  externalProvider?: ExternalMemoryProviderStatus;
  evaluation: MemoryEvalReport;
}

export const MAX_MEMORY_EDIT_CHARS = 8_000;
export const MAX_MEMORY_TAGS = 20;
export const MAX_MEMORY_TAG_CHARS = 40;
export const MAX_FACT_VALUE_CHARS = 500;
export const MAX_FACTS_SHOWN = 500;

export interface MemoryPatch {
  content?: string;
  importance?: number;
  /** `null` clears the tags; absent keeps them. */
  tags?: string[] | null;
}

export interface MemoryDeleteResult {
  id: string;
  removedRelationships: number;
  updatedRelationships: number;
  updatedFacts: number;
}

export type MemoryFactStatus = 'active' | 'superseded' | 'retracted';

/** A temporal fact. Untrusted text: the companion wrote it, so show it as text. */
export interface MemoryFact {
  id: string;
  subjectKind: RelationshipEndpointKind;
  subjectId: string;
  subjectName: string;
  predicate: string;
  objectKind: RelationshipEndpointKind | null;
  objectId: string | null;
  objectName: string | null;
  value: string | null;
  validFrom: number | null;
  validTo: number | null;
  observedAt: number;
  confidence: number;
  evidenceMemoryIds: string[];
  supersedesFactIds: string[];
  status: MemoryFactStatus;
  tags: string[] | null;
  roomId: string | null;
  worldId: string | null;
  sessionId: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface MemoryFactOptions {
  agentId?: string;
  subject?: string;
  includeInactive?: boolean;
  limit?: number;
}

export interface MemoryFactReplaced {
  fact: MemoryFact;
  superseded: MemoryFact;
}

export interface MemoryEntityDeleteResult {
  kind: RelationshipEndpointKind;
  id: string;
  removedRelationships: number;
  removedFacts: number;
}

export class MemoriesClient {
  constructor(private readonly client: DaemonClient) {}

  async create(input: CreateMemoryInput): Promise<Memory> {
    return this.client.requestJson<Memory>('/api/memories', {
      method: 'POST',
      body: input,
    });
  }

  async search(
    query: string,
    options: MemorySearchOptions = {},
  ): Promise<MemorySearchResult[]> {
    const search = new URLSearchParams();
    search.set('q', query);

    if (options.agentId !== undefined) {
      search.set('agentId', options.agentId);
    }
    if (options.agentName !== undefined) {
      search.set('agentName', options.agentName);
    }
    if (options.type !== undefined) {
      search.set('type', options.type);
    }
    if (options.scope !== undefined) {
      search.set('scope', options.scope);
    }
    if (options.roomId !== undefined) {
      search.set('roomId', options.roomId);
    }
    if (options.worldId !== undefined) {
      search.set('worldId', options.worldId);
    }
    if (options.sessionId !== undefined) {
      search.set('sessionId', options.sessionId);
    }
    if (options.limit !== undefined) {
      search.set('limit', String(options.limit));
    }
    if (options.minImportance !== undefined) {
      search.set('minImportance', String(options.minImportance));
    }

    const response = await this.client.requestJson<{
      results: MemorySearchResult[];
    }>(`/api/memories/search?${search.toString()}`);

    return response.results;
  }

  async recent(options: RecentMemoriesOptions = {}): Promise<Memory[]> {
    const search = new URLSearchParams();

    if (options.agentId !== undefined) {
      search.set('agentId', options.agentId);
    }
    if (options.agentName !== undefined) {
      search.set('agentName', options.agentName);
    }
    if (options.scope !== undefined) {
      search.set('scope', options.scope);
    }
    if (options.roomId !== undefined) {
      search.set('roomId', options.roomId);
    }
    if (options.worldId !== undefined) {
      search.set('worldId', options.worldId);
    }
    if (options.sessionId !== undefined) {
      search.set('sessionId', options.sessionId);
    }
    if (options.limit !== undefined) {
      search.set('limit', String(options.limit));
    }

    const path = search.size
      ? `/api/memories/recent?${search.toString()}`
      : '/api/memories/recent';
    const response = await this.client.requestJson<{ memories: Memory[] }>(
      path,
    );

    return response.memories;
  }

  async createEntity(input: CreateMemoryEntityInput): Promise<MemoryEntity> {
    return this.client.requestJson<MemoryEntity>('/api/memories/entities', {
      method: 'POST',
      body: input,
    });
  }

  async entities(options: MemoryEntityOptions = {}): Promise<MemoryEntity[]> {
    const search = new URLSearchParams();

    if (options.entityId !== undefined) {
      search.set('entityId', options.entityId);
    }
    if (options.kind !== undefined) {
      search.set('kind', options.kind);
    }
    if (options.name !== undefined) {
      search.set('name', options.name);
    }
    if (options.alias !== undefined) {
      search.set('alias', options.alias);
    }
    if (options.limit !== undefined) {
      search.set('limit', String(options.limit));
    }

    const path = search.size
      ? `/api/memories/entities?${search.toString()}`
      : '/api/memories/entities';
    const response = await this.client.requestJson<{
      entities: MemoryEntity[];
    }>(path);

    return response.entities;
  }

  async update(memoryId: string, patch: MemoryPatch): Promise<Memory> {
    return this.client.requestJson<Memory>(
      `/api/memories/${encodeURIComponent(memoryId)}`,
      { method: 'PATCH', body: patch },
    );
  }

  async delete(
    memoryId: string,
    expectedContent?: string,
  ): Promise<MemoryDeleteResult> {
    return this.client.requestJson<MemoryDeleteResult>(
      `/api/memories/${encodeURIComponent(memoryId)}`,
      expectedContent === undefined
        ? { method: 'DELETE' }
        : { method: 'DELETE', body: { expectedContent } },
    );
  }

  async facts(options: MemoryFactOptions = {}): Promise<MemoryFact[]> {
    const search = new URLSearchParams();

    if (options.agentId !== undefined) {
      search.set('agentId', options.agentId);
    }
    if (options.subject !== undefined) {
      search.set('subject', options.subject);
    }
    if (options.includeInactive === true) {
      search.set('includeInactive', 'true');
    }
    if (options.limit !== undefined) {
      search.set('limit', String(options.limit));
    }

    const path = search.size
      ? `/api/memories/facts?${search.toString()}`
      : '/api/memories/facts';
    const response = await this.client.requestJson<{ facts: MemoryFact[] }>(
      path,
    );

    return response.facts;
  }

  async coreMemory(
    agentId: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<CoreMemory> {
    return this.client.requestJson<CoreMemory>(
      `${agentPath(agentId)}/core-memory`,
      { signal: options.signal },
    );
  }

  async addCoreEntry(
    agentId: string,
    block: CoreMemoryBlockName,
    text: string,
  ): Promise<CoreMemoryEdit> {
    return this.client.requestJson<CoreMemoryEdit>(
      `${agentPath(agentId)}/core-memory/${block}`,
      { method: 'POST', body: { text } },
    );
  }

  async updateCoreEntry(
    agentId: string,
    block: CoreMemoryBlockName,
    entryId: string,
    text: string,
  ): Promise<CoreMemoryEdit> {
    return this.client.requestJson<CoreMemoryEdit>(
      `${agentPath(agentId)}/core-memory/${block}/${encodeURIComponent(entryId)}`,
      { method: 'PATCH', body: { text } },
    );
  }

  async removeCoreEntry(
    agentId: string,
    block: CoreMemoryBlockName,
    entryId: string,
  ): Promise<CoreMemoryEdit> {
    return this.client.requestJson<CoreMemoryEdit>(
      `${agentPath(agentId)}/core-memory/${block}/${encodeURIComponent(entryId)}`,
      { method: 'DELETE' },
    );
  }

  /** The owner's memory as a downloadable file: JSON (default) or Markdown. */
  async exportMemories(options: MemoryExportOptions = {}): Promise<Blob> {
    const search = new URLSearchParams();
    if (options.format !== undefined) {
      search.set('format', options.format);
    }
    if (options.agentId !== undefined) {
      search.set('agentId', options.agentId);
    }
    const path = search.size
      ? `/api/memories/export?${search.toString()}`
      : '/api/memories/export';
    return this.client.requestBlob(path);
  }

  async replaceFact(
    factId: string,
    value: string,
  ): Promise<MemoryFactReplaced> {
    return this.client.requestJson<MemoryFactReplaced>(
      `/api/memories/facts/${encodeURIComponent(factId)}`,
      { method: 'PATCH', body: { value } },
    );
  }

  async deleteFact(factId: string): Promise<{ id: string }> {
    return this.client.requestJson<{ id: string }>(
      `/api/memories/facts/${encodeURIComponent(factId)}`,
      { method: 'DELETE' },
    );
  }

  async deleteEntity(
    kind: RelationshipEndpointKind,
    entityId: string,
  ): Promise<MemoryEntityDeleteResult> {
    const search = new URLSearchParams({ kind });
    return this.client.requestJson<MemoryEntityDeleteResult>(
      `/api/memories/entities/${encodeURIComponent(
        entityId,
      )}?${search.toString()}`,
      { method: 'DELETE' },
    );
  }

  async evaluate(input: EvaluatedMemoryInput): Promise<MemoryEvaluation> {
    return this.client.requestJson<MemoryEvaluation>(
      '/api/memories/evaluations',
      {
        method: 'POST',
        body: input,
      },
    );
  }

  async addEvaluated(
    input: EvaluatedMemoryInput,
  ): Promise<MemoryEvaluationOutcome> {
    return this.client.requestJson<MemoryEvaluationOutcome>(
      '/api/memories/evaluated',
      {
        method: 'POST',
        body: input,
      },
    );
  }

  async recall(
    query: string,
    options: MemoryRecallOptions = {},
  ): Promise<MemoryRecallResult[]> {
    const search = new URLSearchParams();
    search.set('q', query);

    if (options.agentId !== undefined) {
      search.set('agentId', options.agentId);
    }
    if (options.agentName !== undefined) {
      search.set('agentName', options.agentName);
    }
    if (options.type !== undefined) {
      search.set('type', options.type);
    }
    if (options.scope !== undefined) {
      search.set('scope', options.scope);
    }
    if (options.roomId !== undefined) {
      search.set('roomId', options.roomId);
    }
    if (options.worldId !== undefined) {
      search.set('worldId', options.worldId);
    }
    if (options.sessionId !== undefined) {
      search.set('sessionId', options.sessionId);
    }
    if (options.limit !== undefined) {
      search.set('limit', String(options.limit));
    }
    if (options.minImportance !== undefined) {
      search.set('minImportance', String(options.minImportance));
    }
    if (options.entityId !== undefined) {
      search.set('entityId', options.entityId);
    }
    if (options.recallAgentId !== undefined) {
      search.set('recallAgentId', options.recallAgentId);
    }
    if (options.lexicalLimit !== undefined) {
      search.set('lexicalLimit', String(options.lexicalLimit));
    }
    if (options.recentLimit !== undefined) {
      search.set('recentLimit', String(options.recentLimit));
    }
    if (options.relationshipLimit !== undefined) {
      search.set('relationshipLimit', String(options.relationshipLimit));
    }
    if (options.temporalLimit !== undefined) {
      search.set('temporalLimit', String(options.temporalLimit));
    }

    const response = await this.client.requestJson<{
      results: MemoryRecallResult[];
    }>(`/api/memories/recall?${search.toString()}`);

    return response.results;
  }

  async trace(memoryId: string): Promise<MemoryEvidenceTrace> {
    return this.client.requestJson<MemoryEvidenceTrace>(
      `/api/memories/${encodeURIComponent(memoryId)}/trace`,
    );
  }

  async applyRetention(
    input: MemoryRetentionInput,
  ): Promise<MemoryRetentionReport> {
    return this.client.requestJson<MemoryRetentionReport>(
      '/api/memories/retention',
      {
        method: 'POST',
        body: input,
      },
    );
  }

  async readiness(): Promise<MemoryReadiness> {
    return this.client.requestJson<MemoryReadiness>('/api/memories/readiness');
  }

  async createRelationship(
    input: CreateAgentRelationshipInput,
  ): Promise<AgentRelationship> {
    return this.client.requestJson<AgentRelationship>(
      '/api/memories/relationships',
      {
        method: 'POST',
        body: input,
      },
    );
  }

  async relationships(
    options: AgentRelationshipOptions = {},
  ): Promise<AgentRelationship[]> {
    const search = new URLSearchParams();

    if (options.agentId !== undefined) {
      search.set('agentId', options.agentId);
    }
    if (options.entityId !== undefined) {
      search.set('entityId', options.entityId);
    }
    if (options.sourceKind !== undefined) {
      search.set('sourceKind', options.sourceKind);
    }
    if (options.sourceAgentId !== undefined) {
      search.set('sourceAgentId', options.sourceAgentId);
    }
    if (options.targetKind !== undefined) {
      search.set('targetKind', options.targetKind);
    }
    if (options.targetAgentId !== undefined) {
      search.set('targetAgentId', options.targetAgentId);
    }
    if (options.relationshipType !== undefined) {
      search.set('relationshipType', options.relationshipType);
    }
    if (options.roomId !== undefined) {
      search.set('roomId', options.roomId);
    }
    if (options.worldId !== undefined) {
      search.set('worldId', options.worldId);
    }
    if (options.sessionId !== undefined) {
      search.set('sessionId', options.sessionId);
    }
    if (options.minStrength !== undefined) {
      search.set('minStrength', String(options.minStrength));
    }
    if (options.minConfidence !== undefined) {
      search.set('minConfidence', String(options.minConfidence));
    }
    if (options.limit !== undefined) {
      search.set('limit', String(options.limit));
    }

    const path = search.size
      ? `/api/memories/relationships?${search.toString()}`
      : '/api/memories/relationships';
    const response = await this.client.requestJson<{
      relationships: AgentRelationship[];
    }>(path);

    return response.relationships;
  }
}

function agentPath(agentId: string): string {
  return `/api/agents/${encodeURIComponent(agentId)}`;
}

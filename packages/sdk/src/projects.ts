import type { DaemonClient } from './client.js';
import type { ProjectAddConnectedSourceInput, ProjectConnectedSourceChoices, ProjectConnectedSourceGrant } from './project-connected-sources.js';

export interface ProjectMember { agentId: string; generation: number; joinedAtMs: number }
export type ProjectStatus = 'active' | 'archived';
export interface ProjectSummary {
  id: string; title: string; status: ProjectStatus; revision: number; accessEpoch: number;
  members: ProjectMember[]; createdAtMs: number; updatedAtMs: number;
  chatCount: number; resourceCount: number;
}
/** Targets accepted by the ordinary resource-link endpoint. */
export type ProjectLinkTarget =
  | { kind: 'workspace_file'; path: string }
  | { kind: 'chat_excerpt'; ownerAgentId: string; sessionId: string; clearGeneration: number; messageIds: string[] }
  | { kind: 'chat_result'; ownerAgentId: string; sessionId: string; clearGeneration: number; resultId: string }
  | { kind: 'workflow'; ownerAgentId: string; workflowId: string }
  | { kind: 'workflow_template'; ownerAgentId: string; slug: string };
export type ProjectTarget = ProjectLinkTarget
  | { kind: 'knowledge_folder'; rootPath: string; rootIdentity: string }
  | { kind: 'connected_source'; grant: ProjectConnectedSourceGrant };
export type ProjectResourceState = 'ready' | 'missing' | 'not_in_project' | 'access_removed'
  | 'source_changed' | 'workspace_changed' | 'excluded' | 'unsafe_content' | 'unsupported'
  | 'unavailable' | 'no_matches' | 'budget_omitted' | 'already_in_current_chat';
export interface ProjectResource {
  id: string; target: ProjectTarget | null; state: ProjectResourceState;
  role: 'reference' | 'output'; label: string | null; useInContext: boolean; createdAtMs: number;
}
export interface Project extends ProjectSummary { instructions: string; resources: ProjectResource[] }
export interface ProjectContextReport {
  projectId: string; projectRevision: number; accessEpoch: number; memberGeneration: number;
  estimatedTokens: number; basis: 'current_preview' | 'admitted'; runId?: string;
  resources: { resourceId: string; state: ProjectResourceState; includedChars: number; shortened: boolean }[];
}
export interface ProjectSourcePreview {
  projectId: string; projectRevision: number; accessEpoch: number; memberGeneration: number;
  resourceId: string; state: ProjectResourceState; text: string | null;
  workflow?: { ownerAgentId: string; id: string; title: string; status: string };
}
export interface ProjectChatLink {
  ownerAgentId: string; sessionId: string; title: string; archived: boolean;
  lastActivityAtMs: number; activeRuns: number; projectBindingRevision: number;
}
export interface ProjectChatPage {
  chats: ProjectChatLink[]; nextCursor: string | null;
  projectRevision: number; accessEpoch: number; memberGeneration: number;
}
export interface ProjectCreateInput {
  requestKey: string; title: string; memberAgentIds: string[]; instructions?: string;
}
export interface ProjectPatchInput {
  expectedRevision: number; title?: string; instructions?: string; status?: ProjectStatus;
}
export interface ProjectAddResourceInput {
  expectedRevision: number; requestKey: string; asAgentId: string; target: ProjectLinkTarget;
  role: 'reference' | 'output'; label?: string; useInContext: boolean;
}
export interface ProjectResourcePatch {
  expectedRevision: number; asAgentId: string; label?: string;
  role?: 'reference' | 'output'; useInContext?: boolean;
}
export interface ProjectMembersResult {
  projectSummary: ProjectSummary; accessEpoch: number; detachedChats: number; stopRequestedRunIds: string[];
}
export interface ProjectKnowledgeBinding {
  projectRevision: number; accessEpoch: number; memberGeneration: number;
}
export interface ProjectAddKnowledgeFolderInput {
  expectedRevision: number; requestKey: string; asAgentId: string; rootPath: string; label?: string;
}
export interface ProjectKnowledgeSource {
  resourceId: string; status: 'indexing' | 'ready' | 'partial' | 'unavailable';
  documentCount: number; indexedAtMs: number | null; generation: number; omittedCount: number; error?: string;
}
export interface ProjectKnowledgeStatus extends ProjectKnowledgeBinding { sources: ProjectKnowledgeSource[] }
export interface ProjectKnowledgeHit {
  citationId: string; resourceId: string; documentId: string; title: string; locator: string; excerpt: string;
  origin?: { ownerAgentId: string; connectorId: string; provider: string; remoteId: string;
    ownerName?: string; accountLabel?: string | null; revisionSha256: string; verifiedAtMs: number; coverage: string };
}
export interface ProjectKnowledgeSearch extends ProjectKnowledgeStatus { hits: ProjectKnowledgeHit[] }
export interface ProjectKnowledgeCitation extends ProjectKnowledgeBinding, ProjectKnowledgeHit { text: string }

/** Profile-owned groupings. Member selectors never replace owner authentication. */
export class ProjectsClient {
  constructor(private readonly client: DaemonClient) {}

  async list(memberAgentId: string, options: { status?: ProjectStatus; signal?: AbortSignal } = {}): Promise<ProjectSummary[]> {
    const query = new URLSearchParams({ memberAgentId, status: options.status ?? 'active' });
    const result = await this.client.requestJson<{ projects: ProjectSummary[] }>(`${path()}?${query}`, { signal: options.signal });
    return result.projects;
  }

  async manage(options: { signal?: AbortSignal } = {}): Promise<ProjectSummary[]> {
    const result = await this.client.requestJson<{ projects: ProjectSummary[] }>(`${path()}/manage`, options);
    return result.projects;
  }

  async get(id: string, asAgentId: string, options: { signal?: AbortSignal } = {}): Promise<Project> {
    const result = await this.client.requestJson<{ project: Project }>(memberPath(id, asAgentId), options);
    return result.project;
  }

  async create(input: ProjectCreateInput): Promise<Project> {
    const result = await this.client.requestJson<{ project: Project }>(path(), { method: 'POST', body: input });
    return result.project;
  }

  async update(id: string, patch: ProjectPatchInput): Promise<ProjectSummary> {
    const result = await this.client.requestJson<{ projectSummary: ProjectSummary }>(path(id), { method: 'PATCH', body: patch });
    return result.projectSummary;
  }

  setMembers(id: string, input: { expectedRevision: number; agentIds: string[] }): Promise<ProjectMembersResult> {
    return this.client.requestJson(`${path(id)}/members`, { method: 'PUT', body: input });
  }

  remove(id: string, input: { expectedRevision: number }): Promise<{ deleted: true; detachedChatCount: number }> {
    return this.client.requestJson(path(id), { method: 'DELETE', body: input });
  }

  async addResource(id: string, input: ProjectAddResourceInput): Promise<Project> {
    const result = await this.client.requestJson<{ project: Project }>(`${path(id)}/resources`, { method: 'POST', body: input });
    return result.project;
  }

  async updateResource(id: string, resourceId: string, input: ProjectResourcePatch): Promise<Project> {
    const result = await this.client.requestJson<{ project: Project }>(resourcePath(id, resourceId), { method: 'PATCH', body: input });
    return result.project;
  }

  async removeResource(id: string, resourceId: string, input: { expectedRevision: number; asAgentId: string }): Promise<Project> {
    const result = await this.client.requestJson<{ project: Project }>(resourcePath(id, resourceId), { method: 'DELETE', body: input });
    return result.project;
  }

  async reorderResources(id: string, input: { expectedRevision: number; asAgentId: string; resourceIds: string[] }): Promise<Project> {
    const result = await this.client.requestJson<{ project: Project }>(`${path(id)}/resource-order`, { method: 'PUT', body: input });
    return result.project;
  }

  previewResource(id: string, resourceId: string, asAgentId: string, options: { signal?: AbortSignal } = {}): Promise<ProjectSourcePreview> {
    return this.client.requestJson(`${resourcePath(id, resourceId)}/preview?${new URLSearchParams({ asAgentId })}`, options);
  }

  context(id: string, asAgentId: string, options: { signal?: AbortSignal } = {}): Promise<ProjectContextReport> {
    return this.client.requestJson(`${path(id)}/context?${new URLSearchParams({ asAgentId })}`, options);
  }

  /** Explicit owner grant. Root identities are issued and verified by the host. */
  async addKnowledgeFolder(id: string, input: ProjectAddKnowledgeFolderInput): Promise<Project> {
    const result = await this.client.requestJson<{ project: Project }>(`${path(id)}/knowledge/sources`, { method: 'POST', body: input });
    return result.project;
  }

  connectedSourceChoices(id: string, asAgentId: string, options: { query?: string; signal?: AbortSignal } = {}): Promise<ProjectConnectedSourceChoices> {
    return this.client.requestJson(`${path(id)}/knowledge/connected-sources?${new URLSearchParams({ asAgentId, query: options.query ?? '' })}`, { signal: options.signal });
  }

  async addConnectedSource(id: string, input: ProjectAddConnectedSourceInput): Promise<Project> {
    const result = await this.client.requestJson<{ project: Project }>(`${path(id)}/knowledge/connected-sources`, { method: 'POST', body: input });
    return result.project;
  }

  knowledgeStatus(id: string, asAgentId: string, options: { signal?: AbortSignal } = {}): Promise<ProjectKnowledgeStatus> {
    return this.client.requestJson(`${path(id)}/knowledge/status?${new URLSearchParams({ asAgentId })}`, options);
  }

  refreshKnowledge(id: string, input: { asAgentId: string; resourceId?: string }, options: { signal?: AbortSignal } = {}): Promise<ProjectKnowledgeStatus> {
    return this.client.requestJson(`${path(id)}/knowledge/refresh`, { ...options, method: 'POST', body: input });
  }

  searchKnowledge(id: string, asAgentId: string, options: { query: string; limit?: number; signal?: AbortSignal }): Promise<ProjectKnowledgeSearch> {
    const query = new URLSearchParams({ asAgentId, query: options.query });
    if (options.limit !== undefined) query.set('limit', String(options.limit));
    return this.client.requestJson(`${path(id)}/knowledge/search?${query}`, { signal: options.signal });
  }

  openKnowledgeCitation(id: string, citationId: string, asAgentId: string, options: { signal?: AbortSignal } = {}): Promise<ProjectKnowledgeCitation> {
    return this.client.requestJson(`${path(id)}/knowledge/citations/${encodeURIComponent(citationId)}?${new URLSearchParams({ asAgentId })}`, options);
  }

  chats(id: string, asAgentId: string, options: { cursor?: string; limit?: number; signal?: AbortSignal } = {}): Promise<ProjectChatPage> {
    const query = new URLSearchParams({ asAgentId });
    if (options.cursor) query.set('cursor', options.cursor);
    if (options.limit !== undefined) query.set('limit', String(options.limit));
    return this.client.requestJson(`${path(id)}/chats?${query}`, { signal: options.signal });
  }
}

function path(id?: string): string { return `/api/projects${id ? `/${encodeURIComponent(id)}` : ''}`; }
function memberPath(id: string, asAgentId: string): string { return `${path(id)}?${new URLSearchParams({ asAgentId })}`; }
function resourcePath(id: string, resourceId: string): string { return `${path(id)}/resources/${encodeURIComponent(resourceId)}`; }

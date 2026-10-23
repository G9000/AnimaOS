import { DaemonHttpError } from './client.js';
import type { DaemonClient } from './client.js';
import type { UsageTotals } from './usage.js';
import type { UploadedAttachment } from './attachments.js';
import type { WebSource } from './events.js';
import type { Run } from './runs.js';

export type SessionKind =
  'chat' | 'telegram' | 'discord' | 'email' | 'checkin' | 'job' | 'helper';
export type SessionOrigin =
  | 'web'
  | 'api'
  | 'telegram'
  | 'discord'
  | 'email'
  | 'schedule'
  | 'job'
  | 'delegation'
  | 'peer'
  | 'import';
export type SessionTitleSource =
  'first_message' | 'generated' | 'owner' | 'system';

export interface SessionCapabilities {
  send: boolean;
  steer: boolean;
  stop: boolean;
  rename: boolean;
  archive: boolean;
  delete: boolean;
  compact: boolean;
  export: boolean;
}

export interface SessionSummary {
  text: string;
  throughMessageId: string;
  createdAtMs: number;
  sourceMessageCount: number;
}

export interface SessionContextTrimmed {
  droppedThroughMessageId: string;
  atMs: number;
}

/** The last failed compaction; cleared by the next success (spec §5.4). */
export interface SessionCompactionError {
  message: string;
  atMs: number;
}

/** Estimated selected conversation history and summary, excluding other prompt parts. */
export interface SessionContextUsage {
  estimatedTokens: number;
  /** Usable conversation budget after reserving the reply. */
  budgetTokens: number;
  replyReserveTokens: number;
  /** Estimates for prompt contributions; null means unavailable. */
  breakdown?: {
    conversationTokens: number;
    summaryTokens: number;
    checkpointTokens: number;
    systemInstructionsTokens: number;
    toolSchemasTokens: number;
    companionFilesTokens: number | null;
    memoryTokens: number | null;
  };
  coverage?: {
    basis: string;
    omitted: string[];
    companionFilesRunId?: string;
  };
}

export interface SessionCompactInput {
  /** Number of latest user exchanges to retain, from 1 to 100. */
  keepLatestExchanges?: number;
  /** Untrusted summarization guidance, at most 500 characters. */
  focus?: string;
}

export interface SessionReviseInput {
  mode: 'branch' | 'undo' | 'retry';
  throughMessageId?: string;
}

export interface SessionRevision {
  session: Session;
  /** Restore as a draft only; sending again may repeat completed actions. */
  retryPrompt: string | null;
  attachmentsOmitted: boolean;
  notice: string;
}

export interface SessionQueuedRunPatch {
  text?: string;
  /** Omit to retain order; null moves to the end. */
  beforeRunId?: string | null;
}

export interface SessionMessageSearchHit {
  sessionId: string;
  title: string;
  messageId: string;
  role: 'user' | 'assistant' | 'tool';
  createdAtMs: number;
  snippet: string;
}

export interface SessionMessageSearchPage {
  results: SessionMessageSearchHit[];
  hasMore: boolean;
}

export interface SessionMessageSearchOptions {
  q: string;
  limit?: number;
  signal?: AbortSignal;
}

/** Why a search result matched; `messageId` is null for a title match. */
export interface SessionMatch {
  messageId: string | null;
  snippet: string;
}

export interface SessionImportedFrom {
  tool: 'claude-code' | 'codex';
  foreignSessionId: string | null;
  pathHash: string;
  cwd: string | null;
}

/** A session with the daemon's derived fields (spec §3.2). */
export interface Session {
  id: string;
  agentId: string;
  /** The transcript room; equals `id` except for mapped legacy rooms. */
  roomId: string;
  kind: SessionKind;
  origin: SessionOrigin;
  title: string;
  titleSource: SessionTitleSource;
  createdAtMs: number;
  lastActivityAtMs: number;
  lastReadAtMs: number | null;
  archived: boolean;
  /** The daemon-designated primary web chat; absent on older records. */
  mainChat?: boolean;
  /** Changes when the same conversation identity is cleared. */
  clearGeneration?: number;
  parentSessionId: string | null;
  parentRunId: string | null;
  parentAgentId: string | null;
  summary: SessionSummary | null;
  contextTrimmed: SessionContextTrimmed | null;
  compactionError: SessionCompactionError | null;
  messageCount: number;
  preview: string | null;
  activeRuns: number;
  pendingApprovals: number;
  unread: boolean;
  capabilities: SessionCapabilities;
  /** Where an imported conversation came from (origin `import`). */
  importedFrom?: SessionImportedFrom | null;
  match?: SessionMatch;
  /** The session's usage totals; present only on a single-session read. */
  usage?: UsageTotals | null;
  /** Only on a single-session read; absent on older daemons. */
  contextUsage?: SessionContextUsage;
}

export interface SessionPage {
  sessions: Session[];
  nextCursor: string | null;
}

export interface SessionListOptions {
  kind?: SessionKind;
  /** true lists only archived sessions; the daemon default is false. */
  archived?: boolean;
  q?: string;
  cursor?: string;
  /** 1–200; the daemon default is 50. */
  limit?: number;
  /** The daemon default is true. */
  includeHelpers?: boolean;
  signal?: AbortSignal;
}

export interface SessionMessageAttachment {
  type: 'file' | 'image' | 'url';
  name: string;
  /** Absent on legacy attachment entries, which remain display-only. */
  id?: string;
  mime?: string;
  size?: number;
  kind?: UploadedAttachment['kind'];
  path?: string;
  createdAtMs?: number;
  /** Static, content-free extraction evidence for this user-message attachment. */
  extraction?: SessionMessageDocumentExtraction;
}

export type DocumentExtractionStatus =
  | 'ready'
  | 'partial'
  | 'unsupported'
  | 'encrypted'
  | 'malformed'
  | 'too_large'
  | 'no_text'
  | 'unavailable'
  | 'cancelled'
  | 'context_omitted';

export type DocumentOmission =
  | 'main_body_only'
  | 'paragraph_limit'
  | 'text_limit'
  | 'context_limit'
  | 'selection_limit';

export interface SessionMessageDocxExtraction {
  version: 1;
  status: DocumentExtractionStatus;
  paragraphCount: number;
  omissions: DocumentOmission[];
}

export type PdfDocumentOmission =
  'text_layer_only' | 'text_limit' | 'context_limit' | 'selection_limit';

export interface SessionMessagePdfExtraction {
  version: 2;
  format: 'pdf';
  status: DocumentExtractionStatus;
  pageCount: number | null;
  textPageCount: number | null;
  includedPageCount: number | null;
  omissions: PdfDocumentOmission[];
}

export type XlsxDocumentOmission =
  | 'stored_cells_only'
  | 'cell_limit'
  | 'cell_text_limit'
  | 'text_limit'
  | 'context_limit'
  | 'selection_limit';

/** Counts only; formulas are not evaluated and cached values may be stale. */
export interface SessionMessageXlsxExtraction {
  version: 2;
  format: 'xlsx';
  status: DocumentExtractionStatus;
  sheetCount: number | null;
  observedCellCount: number | null;
  includedCellCount: number | null;
  /** Includes both hidden and very-hidden sheets. */
  hiddenSheetCount: number | null;
  omissions: XlsxDocumentOmission[];
}

/** Closed public summaries for the attachment's extracted document text. */
export type SessionMessageDocumentExtraction =
  | SessionMessageDocxExtraction
  | SessionMessagePdfExtraction
  | SessionMessageXlsxExtraction;

export interface SessionMessageMetadata extends Record<string, unknown> {
  /** Narrow host projection, present only on successful native search results. */
  webSources?: WebSource[];
}

export interface SessionMessage {
  id: string;
  role: 'user' | 'assistant' | 'system' | 'tool';
  text: string;
  attachments: SessionMessageAttachment[];
  metadata: SessionMessageMetadata;
  createdAtMs: number;
  /** Present on silent check-in messages when `includeHidden` is set. */
  hidden?: boolean;
}

export interface SessionMessagePage {
  /** Oldest to newest within the page. */
  messages: SessionMessage[];
  /**
   * The `before` cursor for the next (older) page, or null once history is
   * exhausted. A page degraded by an unreadable history store returns a
   * cursor here instead of null even when the hot tail itself is exhausted;
   * requesting that cursor then fails with a 503 rather than the page
   * quietly claiming there is nothing more.
   */
  nextBefore: string | null;
  /** Present only for an exact bounded historical window. */
  anchorMessageId?: string;
  hasNewer?: boolean;
}

export interface SessionMessageOptions {
  before?: string;
  /** Mutually exclusive with before and includeHidden=true. */
  anchor?: string;
  limit?: number;
  includeHidden?: boolean;
  signal?: AbortSignal;
}

export interface SessionUpdateInput {
  title?: string;
  archived?: boolean;
  lastReadAtMs?: number;
}

/**
 * Thrown by `SessionsClient.list` and `main`. The daemon answers an unknown route
 * with the same JSON 404 as a real not-found, so a 404 from `GET
 * /api/agents/{id}/sessions` alone cannot tell "the daemon predates the
 * sessions routes" apart from "the agent does not exist" (Controller ruling
 * 2, M2 pre-flight audit; spec §13.4). Both operations resolve the ambiguity by
 * probing `GET /api/agents/{id}`: if the agent exists, the sessions route is
 * the thing missing, so it raises this instead of the plain `DaemonHttpError`.
 * `code` is a stable string a caller can check without importing this class.
 */
export class DaemonTooOldError extends Error {
  readonly code = 'daemon_too_old' as const;

  constructor(
    readonly agentId: string,
    operation = 'sessions',
  ) {
    super(
      `The daemon does not support ${operation} for agent "${agentId}" yet; update the daemon.`,
    );
    this.name = 'DaemonTooOldError';
  }
}

export class SessionsClient {
  constructor(private readonly client: DaemonClient) {}

  async searchMessages(
    agentId: string,
    options: SessionMessageSearchOptions,
  ): Promise<SessionMessageSearchPage> {
    const search = new URLSearchParams({ q: options.q });
    if (options.limit !== undefined) search.set('limit', String(options.limit));
    return this.client.requestJson<SessionMessageSearchPage>(
      withQuery(
        `/api/agents/${encodeURIComponent(agentId)}/message-search`,
        search,
      ),
      { signal: options.signal },
    );
  }

  /** Atomically resolves the agent's durable main web conversation. */
  async main(agentId: string): Promise<Session> {
    try {
      const response = await this.client.requestJson<{ session: Session }>(
        `/api/agents/${encodeURIComponent(agentId)}/main-chat`,
        { method: 'POST', body: {} },
      );
      return response.session;
    } catch (error) {
      if (
        error instanceof DaemonHttpError &&
        error.status === 404 &&
        (await this.agentExists(agentId))
      ) {
        throw new DaemonTooOldError(agentId, 'main chat');
      }
      throw error;
    }
  }

  async list(
    agentId: string,
    options: SessionListOptions = {},
  ): Promise<SessionPage> {
    const search = new URLSearchParams();
    if (options.kind) search.set('kind', options.kind);
    if (options.archived !== undefined)
      search.set('archived', String(options.archived));
    if (options.q) search.set('q', options.q);
    if (options.cursor) search.set('cursor', options.cursor);
    if (options.limit !== undefined) search.set('limit', String(options.limit));
    if (options.includeHelpers !== undefined)
      search.set('includeHelpers', String(options.includeHelpers));
    try {
      return await this.client.requestJson<SessionPage>(
        withQuery(sessionsPath(agentId), search),
        { signal: options.signal },
      );
    } catch (error) {
      if (error instanceof DaemonHttpError && error.status === 404) {
        if (await this.agentExists(agentId)) {
          throw new DaemonTooOldError(agentId);
        }
      }
      throw error;
    }
  }

  /** Whether `GET /api/agents/{agentId}` answers 200 (Ruling 2, M2 pre-flight audit). */
  private async agentExists(agentId: string): Promise<boolean> {
    try {
      await this.client.agents.get(agentId);
      return true;
    } catch {
      return false;
    }
  }

  async get(
    agentId: string,
    sessionId: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<Session> {
    const response = await this.client.requestJson<{ session: Session }>(
      sessionPath(agentId, sessionId),
      { signal: options.signal },
    );
    return response.session;
  }

  async messages(
    agentId: string,
    sessionId: string,
    options: SessionMessageOptions = {},
  ): Promise<SessionMessagePage> {
    const search = new URLSearchParams();
    if (options.before) search.set('before', options.before);
    if (options.anchor) search.set('anchor', options.anchor);
    if (options.limit !== undefined) search.set('limit', String(options.limit));
    if (options.includeHidden !== undefined)
      search.set('includeHidden', String(options.includeHidden));
    return this.client.requestJson<SessionMessagePage>(
      withQuery(`${sessionPath(agentId, sessionId)}/messages`, search),
      { signal: options.signal },
    );
  }

  async create(
    agentId: string,
    input: { title?: string } = {},
  ): Promise<Session> {
    const response = await this.client.requestJson<{ session: Session }>(
      sessionsPath(agentId),
      { method: 'POST', body: input },
    );
    return response.session;
  }

  async update(
    agentId: string,
    sessionId: string,
    patch: SessionUpdateInput,
  ): Promise<Session> {
    const response = await this.client.requestJson<{ session: Session }>(
      sessionPath(agentId, sessionId),
      { method: 'PATCH', body: patch },
    );
    return response.session;
  }

  async remove(agentId: string, sessionId: string): Promise<void> {
    await this.client.requestJson<{ deleted: boolean }>(
      sessionPath(agentId, sessionId),
      { method: 'DELETE' },
    );
  }

  /** Copies native history into a new chat without changing the source. */
  revise(
    agentId: string,
    sessionId: string,
    input: SessionReviseInput,
  ): Promise<SessionRevision> {
    return this.client.requestJson<SessionRevision>(
      `${sessionPath(agentId, sessionId)}/revise`,
      { method: 'POST', body: input },
    );
  }

  /** Mutates only a waiting, ordinary owner prompt; the daemon rejects races. */
  async updateQueuedRun(
    agentId: string,
    sessionId: string,
    runId: string,
    patch: SessionQueuedRunPatch,
  ): Promise<Run> {
    const response = await this.client.requestJson<{ run: Run }>(
      `${sessionPath(agentId, sessionId)}/queue/${encodeURIComponent(runId)}`,
      { method: 'PATCH', body: patch },
    );
    return response.run;
  }

  /** The full transcript as Markdown, including archived history and
   *  messages kept only in the history store, with silent check-in turns
   *  marked rather than hidden. */
  async exportMarkdown(
    agentId: string,
    sessionId: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<string> {
    return this.client.requestText(
      `${sessionPath(agentId, sessionId)}/export`,
      {
        signal: options.signal,
      },
    );
  }

  /** Folds every uncovered turn but the newest into the session summary
   *  (spec §5.4). The summary arrives later with `session.updated`. */
  async compact(
    agentId: string,
    sessionId: string,
    input?: SessionCompactInput,
  ): Promise<Session> {
    const response = await this.client.requestJson<{ session: Session }>(
      `${sessionPath(agentId, sessionId)}/compact`,
      { method: 'POST', ...(input ? { body: input } : {}) },
    );
    return response.session;
  }
}

function sessionsPath(agentId: string): string {
  return `/api/agents/${encodeURIComponent(agentId)}/sessions`;
}

function sessionPath(agentId: string, sessionId: string): string {
  return `${sessionsPath(agentId)}/${encodeURIComponent(sessionId)}`;
}

function withQuery(path: string, search: URLSearchParams): string {
  const query = search.toString();
  return query ? `${path}?${query}` : path;
}

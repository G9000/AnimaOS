import { AgentsClient } from './agents.js';
import { AgentFilesClient } from './agent-files.js';
import { AttachmentsClient } from './attachments.js';
import { VoiceClient } from './voice.js';
import { SpeechClient } from './speech.js';
import { GoalsClient } from './goals.js';
import { AgenciesClient } from './agencies.js';
import { WorkspaceClient } from './workspace.js';
import { ChatGptClient } from './chatgpt.js';
import { ConnectorsClient } from './connectors.js';
import { MemoriesClient } from './memories.js';
import { SwarmsClient } from './swarms.js';
import { SessionsClient } from './sessions.js';
import { ChatCapabilitiesClient } from './chat-capabilities.js';
import { ChatModels } from './chat-models.js';
import { ChatResultsClient } from './results.js';
import { ConversationSharesClient } from './conversation-shares.js';
import { InteractionsClient } from './interactions.js';
import { RunsClient } from './runs.js';
import { WorkflowsClient, WorkflowTemplatesClient } from './workflows.js';
import { ToolResultsClient } from './tool-results.js';
import { LearningClient } from './learning.js';
import { McpClient } from './mcp.js';
import { CommunityBundlesClient } from './community-bundles.js';
import { BrowserScopesClient } from './browser.js';
import { ApprovalsClient } from './approvals.js';
import { SkillsClient } from './skills.js';
import { AutomationsClient } from './automations.js';
import { UsageClient } from './usage.js';
import { LogsClient } from './logs.js';
import { StatusClient } from './status.js';
import { SystemClient } from './system.js';
import { AgentEventsClient } from './events.js';
import type { DaemonCapabilities } from './capabilities.js';
import { DaemonHttpError } from './daemon-errors.js';

export { DaemonHttpError } from './daemon-errors.js';

const DEFAULT_BASE_URL = 'http://127.0.0.1:8080';

export type FetchLike = typeof fetch;
type RequestHeaders = NonNullable<RequestInit['headers']>;
type RequestBody = NonNullable<RequestInit['body']>;

export interface DaemonClientOptions {
  /** Daemon origin or path prefix. Use an empty string for same-origin browser requests. */
  baseUrl?: string;
  fetch?: FetchLike;
}

export interface DaemonEvent<T = unknown> {
  event: string;
  data: T;
  id?: string;
}

export interface DaemonHealth {
  status: 'ok';
}

export class DaemonConnectionError extends Error {
  override readonly cause: unknown;

  constructor(baseUrl: string, cause: unknown) {
    super(`Failed to reach daemon at ${baseUrl}`);
    this.name = 'DaemonConnectionError';
    this.cause = cause;
  }
}

export class DaemonClient {
  readonly agentFiles: AgentFilesClient;
  readonly attachments: AttachmentsClient;
  readonly voice: VoiceClient;
  readonly speech: SpeechClient;
  readonly agencies: AgenciesClient;
  readonly workspace: WorkspaceClient;
  readonly chatgpt: ChatGptClient;
  readonly agents: AgentsClient;
  readonly goals: GoalsClient;
  readonly connectors: ConnectorsClient;
  readonly memories: MemoriesClient;
  readonly swarms: SwarmsClient;
  readonly sessions: SessionsClient;
  readonly chatCapabilities: ChatCapabilitiesClient;
  readonly chatModels: ChatModels;
  readonly chatResults: ChatResultsClient;
  readonly conversationShares: ConversationSharesClient;
  readonly interactions: InteractionsClient;
  readonly runs: RunsClient;
  readonly workflows: WorkflowsClient;
  readonly workflowTemplates: WorkflowTemplatesClient;
  readonly toolResults: ToolResultsClient;
  readonly learning: LearningClient;
  readonly mcp: McpClient;
  readonly communityBundles: CommunityBundlesClient;
  readonly browserScopes: BrowserScopesClient;
  readonly events: AgentEventsClient;
  readonly approvals: ApprovalsClient;
  readonly skills: SkillsClient;
  readonly automations: AutomationsClient;
  readonly usage: UsageClient;
  readonly logs: LogsClient;
  readonly status: StatusClient;
  readonly system: SystemClient;

  private readonly baseUrl: string;
  private readonly fetchImpl: FetchLike;

  constructor(options: DaemonClientOptions = {}) {
    this.baseUrl = normalizeBaseUrl(options.baseUrl ?? DEFAULT_BASE_URL);
    const fetchImpl = options.fetch ?? globalThis.fetch?.bind(globalThis);

    if (!fetchImpl) {
      throw new Error('fetch is not available; provide a fetch implementation');
    }

    this.fetchImpl = fetchImpl;
    this.agentFiles = new AgentFilesClient(this);
    this.attachments = new AttachmentsClient(this);
    this.voice = new VoiceClient(this);
    this.speech = new SpeechClient(this);
    this.agencies = new AgenciesClient(this);
    this.workspace = new WorkspaceClient(this);
    this.agents = new AgentsClient(this);
    this.goals = new GoalsClient(this);
    this.chatgpt = new ChatGptClient(this);
    this.connectors = new ConnectorsClient(this);
    this.memories = new MemoriesClient(this);
    this.swarms = new SwarmsClient(this);
    this.sessions = new SessionsClient(this);
    this.chatCapabilities = new ChatCapabilitiesClient(this);
    this.chatModels = new ChatModels(this);
    this.chatResults = new ChatResultsClient(this);
    this.conversationShares = new ConversationSharesClient(this);
    this.interactions = new InteractionsClient(this);
    this.runs = new RunsClient(this);
    this.workflows = new WorkflowsClient(this);
    this.workflowTemplates = new WorkflowTemplatesClient(this);
    this.toolResults = new ToolResultsClient(this);
    this.learning = new LearningClient(this);
    this.mcp = new McpClient(this);
    this.communityBundles = new CommunityBundlesClient(this);
    this.browserScopes = new BrowserScopesClient(this);
    this.events = new AgentEventsClient(this);
    this.approvals = new ApprovalsClient(this);
    this.skills = new SkillsClient(this);
    this.automations = new AutomationsClient(this);
    this.usage = new UsageClient(this);
    this.logs = new LogsClient(this);
    this.status = new StatusClient(this);
    this.system = new SystemClient(this);
  }

  async health(): Promise<DaemonHealth> {
    return this.requestJson<DaemonHealth>('/health');
  }

  /** Discover native handlers and planned modules without granting access. */
  async capabilities(): Promise<DaemonCapabilities> {
    return this.requestJson<DaemonCapabilities>('/api/capabilities');
  }

  async requestJson<T>(
    path: string,
    init: Omit<RequestInit, 'body'> & {
      body?: unknown;
    } = {},
  ): Promise<T> {
    const { body, isJson } = prepareRequestBody(init.body);
    const headers: Record<string, string> = {
      accept: 'application/json',
      ...headersToObject(init.headers),
    };
    if (isJson && !('content-type' in headers)) {
      headers['content-type'] = 'application/json';
    }

    const response = await this.fetchWithConnectionErrors(path, {
      ...init,
      headers,
      body,
    });

    const payload = await this.readWithConnectionErrors(path, () =>
      readResponseBody(response),
    );
    if (!response.ok) {
      throw new DaemonHttpError(response.status, payload);
    }

    return payload as T;
  }

  /** A text response such as Markdown; failures throw like `requestJson`. */
  async requestText(path: string, init: RequestInit = {}): Promise<string> {
    const response = await this.fetchWithConnectionErrors(path, {
      ...init,
      headers: {
        accept: 'text/markdown, text/plain;q=0.9, */*;q=0.1',
        ...headersToObject(init.headers),
      },
    });
    if (!response.ok) {
      throw new DaemonHttpError(
        response.status,
        await this.readWithConnectionErrors(path, () =>
          readResponseBody(response),
        ),
      );
    }
    return this.readWithConnectionErrors(path, () => response.text());
  }

  /** Binary bytes; errors retain the same HTTP/connection policy as JSON. */
  async requestBlob(path: string, init: RequestInit = {}): Promise<Blob> {
    const response = await this.fetchWithConnectionErrors(path, {
      ...init,
      headers: {
        accept: '*/*',
        ...headersToObject(init.headers),
      },
    });
    if (!response.ok) {
      throw new DaemonHttpError(
        response.status,
        await this.readWithConnectionErrors(path, () =>
          readResponseBody(response),
        ),
      );
    }
    return this.readWithConnectionErrors(path, () => response.blob());
  }

  /** Read one binary response with a strict decoded-byte ceiling. */
  async requestBoundedBlob(
    path: string,
    init: RequestInit,
    limits: { maxBytes: number; contentType: string; allowEmpty?: boolean },
  ): Promise<Blob> {
    const minimumBytes = limits.allowEmpty === true ? 0 : 1;
    const { body, isJson } = prepareRequestBody(init.body);
    const headers: Record<string, string> = {
      accept: limits.contentType,
      ...headersToObject(init.headers),
    };
    if (isJson && !('content-type' in headers))
      headers['content-type'] = 'application/json';
    const response = await this.fetchWithConnectionErrors(path, {
      ...init,
      headers,
      body,
    });
    if (!response.ok) {
      // The status is all the caller needs; never materialize an unbounded error body.
      await response.body?.cancel().catch(() => undefined);
      throw new DaemonHttpError(response.status, null);
    }
    const declared = response.headers.get('content-length');
    if (
      declared !== null &&
      (!/^\d+$/.test(declared) ||
        Number(declared) < minimumBytes ||
        Number(declared) > limits.maxBytes)
    ) {
      await response.body?.cancel().catch(() => undefined);
      throw new Error('Invalid bounded response length');
    }
    const encoding = response.headers
      .get('content-encoding')
      ?.trim()
      .toLowerCase();
    if (
      response.headers.get('content-type')?.trim().toLowerCase() !==
        limits.contentType ||
      (encoding && encoding !== 'identity') ||
      (!response.body &&
        (minimumBytes > 0 || (declared !== null && Number(declared) !== 0)))
    ) {
      await response.body?.cancel().catch(() => undefined);
      throw new Error('Invalid bounded response type');
    }
    if (!response.body) {
      return new Blob([], { type: limits.contentType });
    }
    const reader = response.body.getReader();
    const chunks: Uint8Array[] = [];
    let total = 0;
    let complete = false;
    try {
      while (true) {
        const { value, done } = await this.readWithConnectionErrors(path, () =>
          reader.read(),
        );
        if (done) break;
        total += value.byteLength;
        if (total > limits.maxBytes)
          throw new Error('Bounded response too large');
        chunks.push(value);
      }
      if (
        total < minimumBytes ||
        (declared !== null && total !== Number(declared))
      )
        throw new Error('Invalid bounded response length');
      complete = true;
      const output = new Uint8Array(total);
      let offset = 0;
      for (const chunk of chunks) {
        output.set(chunk, offset);
        offset += chunk.byteLength;
      }
      return new Blob([output.buffer], { type: limits.contentType });
    } finally {
      if (!complete)
        await reader.cancel('bounded response rejected').catch(() => undefined);
      reader.releaseLock();
    }
  }

  /** Status-only operation; response bodies are always discarded. */
  async requestNoBody(
    path: string,
    init: RequestInit,
    expectedStatus: number,
  ): Promise<void> {
    const response = await this.fetchWithConnectionErrors(path, init);
    await response.body?.cancel().catch(() => undefined);
    if (response.status !== expectedStatus)
      throw new DaemonHttpError(response.status, null);
  }

  async *subscribe<T = unknown>(
    path: string,
    init: RequestInit = {},
  ): AsyncGenerator<DaemonEvent<T>> {
    const abortController = new AbortController();
    const detachAbortRelay = relayAbort(init.signal, abortController);
    let reader: ReadableStreamDefaultReader<Uint8Array> | undefined;

    try {
      const response = await this.fetchWithConnectionErrors(path, {
        ...init,
        method: init.method ?? 'GET',
        headers: {
          accept: 'text/event-stream',
          ...headersToObject(init.headers),
        },
        signal: abortController.signal,
      });

      if (!response.ok) {
        throw new DaemonHttpError(
          response.status,
          await readResponseBody(response),
        );
      }

      if (!response.body) {
        throw new Error('daemon event stream did not include a response body');
      }

      reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = '';

      while (true) {
        const { done, value } = await reader.read();
        if (done) {
          break;
        }

        buffer += decoder.decode(value, { stream: true });
        let separatorIndex = findEventSeparator(buffer);
        while (separatorIndex >= 0) {
          const chunk = buffer.slice(0, separatorIndex);
          buffer = buffer.slice(
            separatorIndex + eventSeparatorLength(buffer, separatorIndex),
          );

          const event = parseSseEvent(chunk);
          if (event) {
            yield event as DaemonEvent<T>;
          }

          separatorIndex = findEventSeparator(buffer);
        }
      }

      buffer += decoder.decode();
      const trailingEvent = parseSseEvent(buffer);
      if (trailingEvent) {
        yield trailingEvent as DaemonEvent<T>;
      }
    } finally {
      abortController.abort();
      detachAbortRelay();
      if (reader) {
        await reader.cancel('subscription closed').catch(() => undefined);
        reader.releaseLock();
      }
    }
  }

  private url(path: string): string {
    return `${this.baseUrl}${path.startsWith('/') ? path : `/${path}`}`;
  }

  private async fetchWithConnectionErrors(
    path: string,
    init: RequestInit,
  ): Promise<Response> {
    try {
      return await this.fetchImpl(this.url(path), init);
    } catch (error) {
      if (error instanceof Error && error.name === 'AbortError') {
        throw error;
      }

      throw new DaemonConnectionError(this.url(path), error);
    }
  }

  /** A body that fails after its headers (the connection dropped midway)
   *  is a connection error too, not a bare TypeError (T18). */
  private async readWithConnectionErrors<T>(
    path: string,
    read: () => Promise<T>,
  ): Promise<T> {
    try {
      return await read();
    } catch (error) {
      if (error instanceof Error && error.name === 'AbortError') {
        throw error;
      }

      throw new DaemonConnectionError(this.url(path), error);
    }
  }
}

export function createDaemonClient(
  options: DaemonClientOptions = {},
): DaemonClient {
  return new DaemonClient(options);
}

function normalizeBaseUrl(baseUrl: string): string {
  return baseUrl.replace(/\/+$/, '');
}

function headersToObject(headers?: RequestHeaders): Record<string, string> {
  if (!headers) {
    return {};
  }

  const normalized: Record<string, string> = {};

  if (headers instanceof Headers) {
    for (const [key, value] of headers.entries()) {
      normalized[key] = value;
    }
    return normalized;
  }

  if (Array.isArray(headers)) {
    for (const [key, value] of headers) {
      normalized[key] = value;
    }
    return normalized;
  }

  for (const [key, value] of Object.entries(
    headers as Record<string, string | readonly string[]>,
  )) {
    normalized[key] = typeof value === 'string' ? value : value.join(', ');
  }

  return normalized;
}

function prepareRequestBody(body: unknown): {
  body: RequestBody | undefined;
  isJson: boolean;
} {
  if (body === undefined || body === null) {
    return {
      body: undefined,
      isJson: false,
    };
  }

  if (
    typeof body === 'string' ||
    body instanceof Blob ||
    body instanceof ArrayBuffer ||
    body instanceof FormData ||
    body instanceof URLSearchParams ||
    body instanceof ReadableStream
  ) {
    return {
      body,
      isJson: false,
    };
  }

  return {
    body: JSON.stringify(body),
    isJson: true,
  };
}

async function readResponseBody(response: Response): Promise<unknown> {
  const text = await response.text();
  if (text.length === 0) {
    return null;
  }

  try {
    return JSON.parse(text) as unknown;
  } catch {
    return text;
  }
}

function findEventSeparator(buffer: string): number {
  const windowsIndex = buffer.indexOf('\r\n\r\n');
  const unixIndex = buffer.indexOf('\n\n');

  if (windowsIndex === -1) {
    return unixIndex;
  }

  if (unixIndex === -1) {
    return windowsIndex;
  }

  return Math.min(windowsIndex, unixIndex);
}

function eventSeparatorLength(buffer: string, index: number): number {
  return buffer.startsWith('\r\n\r\n', index) ? 4 : 2;
}

function parseSseEvent(rawEvent: string): DaemonEvent | null {
  const normalized = rawEvent.replace(/\r\n/g, '\n').trim();
  if (normalized.length === 0) {
    return null;
  }

  let event = 'message';
  let id: string | undefined;
  const dataLines: string[] = [];

  for (const line of normalized.split('\n')) {
    if (line.length === 0 || line.startsWith(':')) {
      continue;
    }

    const separatorIndex = line.indexOf(':');
    const field = separatorIndex === -1 ? line : line.slice(0, separatorIndex);
    const rawValue =
      separatorIndex === -1 ? '' : line.slice(separatorIndex + 1);
    const value = rawValue.startsWith(' ') ? rawValue.slice(1) : rawValue;

    switch (field) {
      case 'event':
        event = value;
        break;
      case 'data':
        dataLines.push(value);
        break;
      case 'id':
        id = value;
        break;
      default:
        break;
    }
  }

  if (dataLines.length === 0) {
    return null;
  }

  const rawData = dataLines.join('\n');

  return {
    event,
    data: parseJsonLike(rawData),
    ...(id ? { id } : {}),
  };
}

function parseJsonLike(value: string): unknown {
  try {
    return JSON.parse(value) as unknown;
  } catch {
    return value;
  }
}

function relayAbort(
  source: AbortSignal | null | undefined,
  target: AbortController,
): () => void {
  if (!source) {
    return () => {};
  }

  if (source.aborted) {
    target.abort();
    return () => {};
  }

  const onAbort = () => {
    target.abort();
  };

  source.addEventListener('abort', onAbort, { once: true });

  return () => {
    source.removeEventListener('abort', onAbort);
  };
}

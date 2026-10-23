import { DaemonHttpError } from './daemon-errors.js';
import type { DaemonClient, FetchLike } from './client.js';

export interface ConversationShareGrant {
  id: string;
  viewerId: string;
  viewerName: string;
  createdAtMs: number;
  revokedAtMs: number | null;
  active: boolean;
}

export interface ConversationShareCreated {
  grant: ConversationShareGrant;
  /** Returned only when issued. Never put this in a share URL. */
  secret: string;
}

export interface SharedConversation {
  id: string;
  title: string;
  viewerName: string;
  readOnly: true;
  attachmentsOmitted: true;
}

export interface SharedConversationMessage {
  id: string;
  role: 'user' | 'assistant';
  text: string;
  createdAtMs: number;
}

export interface SharedConversationPage {
  title: string;
  messages: SharedConversationMessage[];
  nextBefore: string | null;
  attachmentsOmitted: true;
}

/** Owner-only administration, using the caller's normal owner transport. */
export class ConversationSharesClient {
  constructor(private readonly client: DaemonClient) {}

  list(
    agentId: string,
    sessionId: string,
    signal?: AbortSignal,
  ): Promise<{ grants: ConversationShareGrant[] }> {
    return this.client.requestJson(path(agentId, sessionId), { signal });
  }

  create(
    agentId: string,
    sessionId: string,
    input: { viewerName: string; clearGeneration: number },
  ): Promise<ConversationShareCreated> {
    return this.client.requestJson(path(agentId, sessionId), {
      method: 'POST',
      body: input,
    });
  }

  revoke(
    agentId: string,
    sessionId: string,
    shareId: string,
  ): Promise<{ grants: ConversationShareGrant[] }> {
    return this.client.requestJson(
      `${path(agentId, sessionId)}/${encodeURIComponent(shareId)}`,
      { method: 'DELETE' },
    );
  }
}

function path(agentId: string, sessionId: string): string {
  return `/api/agents/${encodeURIComponent(agentId)}/sessions/${encodeURIComponent(sessionId)}/shares`;
}

export interface SharedConversationClientOptions {
  baseUrl?: string;
  viewerId: string;
  secret: string;
  /** A plain fetch implementation, never an owner transport wrapper. */
  fetch?: FetchLike;
}

/** Dedicated viewer transport: no owner client, cookies, redirects or writes. */
export class SharedConversationClient {
  private readonly fetchImpl: FetchLike;
  private readonly baseUrl: string;
  private readonly viewerId: string;
  private readonly secret: string;

  constructor(options: SharedConversationClientOptions) {
    this.baseUrl = (options.baseUrl ?? '').replace(/\/+$/, '');
    this.fetchImpl = options.fetch ?? globalThis.fetch.bind(globalThis);
    this.viewerId = options.viewerId;
    this.secret = options.secret;
  }

  get(shareId: string, signal?: AbortSignal): Promise<SharedConversation> {
    return this.read(shareId, '', signal);
  }

  messages(
    shareId: string,
    options: { before?: string; signal?: AbortSignal } = {},
  ): Promise<SharedConversationPage> {
    const query = options.before
      ? `?before=${encodeURIComponent(options.before)}`
      : '';
    return this.read(shareId, `/messages${query}`, options.signal);
  }

  private async read<T>(
    shareId: string,
    suffix: string,
    signal?: AbortSignal,
  ): Promise<T> {
    const response = await this.fetchImpl(
      `${this.baseUrl}/api/shared/conversations/${encodeURIComponent(shareId)}${suffix}`,
      {
        method: 'GET',
        credentials: 'omit',
        cache: 'no-store',
        redirect: 'error',
        referrerPolicy: 'no-referrer',
        headers: {
          accept: 'application/json',
          'x-anima-viewer-id': this.viewerId,
          'x-anima-viewer-secret': this.secret,
        },
        signal,
      },
    );
    if (!response.ok) {
      await response.body?.cancel().catch(() => undefined);
      throw new DaemonHttpError(response.status, {
        error:
          response.status === 401
            ? 'Viewer access is unavailable. Check your viewer ID and secret, or ask the owner for a new grant.'
            : 'The shared conversation could not be loaded.',
      });
    }
    return response.json() as Promise<T>;
  }
}

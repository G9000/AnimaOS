import type { DaemonClient } from './client.js';

export interface ChatGptLogin {
  userCode: string;
  verificationUrl: string;
  expiresAtMs: number;
}

/** Redacted daemon-owned subscription state. Credentials never leave the host. */
export interface ChatGptStatus {
  connected: boolean;
  accountId: string | null;
  planType: string | null;
  login: ChatGptLogin | null;
  error: string | null;
}

export interface ChatGptModelEntry {
  id: string;
}

/** Account-scoped catalog metadata; presence does not prove generation access. */
export interface ChatGptModelsSnapshot {
  state:
    | 'unrequested'
    | 'ready'
    | 'empty'
    | 'stale'
    | 'unavailable'
    | 'disconnected';
  connectionRevision: string | null;
  catalogRevision: string | null;
  fetchedAtMs: number | null;
  expiresAtMs: number | null;
  models: ChatGptModelEntry[];
  error:
    | 'Model metadata is unavailable.'
    | 'Model metadata request timed out.'
    | 'Model metadata response is invalid.'
    | null;
}

export interface ChatGptModelsOptions {
  signal?: AbortSignal;
}

export class ChatGptClient {
  constructor(private readonly client: DaemonClient) {}

  /** Reads the daemon cache only; never requests upstream metadata. */
  models(options: ChatGptModelsOptions = {}): Promise<ChatGptModelsSnapshot> {
    return this.client.requestJson('/api/providers/chatgpt/models', {
      headers: { 'cache-control': 'no-store' },
      signal: options.signal,
    });
  }

  /** One explicit metadata refresh. No automatic retry or model selection. */
  refreshModels(
    options: ChatGptModelsOptions = {},
  ): Promise<ChatGptModelsSnapshot> {
    return this.client.requestJson('/api/providers/chatgpt/models/refresh', {
      method: 'POST',
      headers: { 'cache-control': 'no-store' },
      body: {},
      signal: options.signal,
    });
  }

  status(): Promise<ChatGptStatus> {
    return this.client.requestJson('/api/providers/chatgpt/status', {
      headers: { 'cache-control': 'no-store' },
    });
  }

  login(): Promise<ChatGptStatus> {
    return this.client.requestJson('/api/providers/chatgpt/login', {
      method: 'POST',
      headers: { 'cache-control': 'no-store' },
    });
  }

  cancel(): Promise<ChatGptStatus> {
    return this.client.requestJson('/api/providers/chatgpt/login', {
      method: 'DELETE',
      headers: { 'cache-control': 'no-store' },
    });
  }

  disconnect(): Promise<ChatGptStatus> {
    return this.client.requestJson('/api/providers/chatgpt', {
      method: 'DELETE',
      headers: { 'cache-control': 'no-store' },
    });
  }
}

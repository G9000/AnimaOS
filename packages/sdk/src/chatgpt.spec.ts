import { describe, expect, it } from 'vitest';
import {
  createDaemonClient,
  DaemonHttpError,
  type ChatGptStatus,
  type ChatGptModelsSnapshot,
} from './index.js';

describe('ChatGPT subscription transport', () => {
  it('m49_chatgpt_models_reads_cache_and_serializes_only_explicit_empty_refresh', async () => {
    const calls: { url: string; init?: RequestInit }[] = [];
    const snapshot: ChatGptModelsSnapshot = {
      state: 'unavailable',
      connectionRevision: 'connection-a',
      catalogRevision: null,
      fetchedAtMs: null,
      expiresAtMs: null,
      models: [],
      error: 'Model metadata is unavailable.',
    };
    const client = createDaemonClient({
      baseUrl: '',
      fetch: async (url, init) => {
        calls.push({ url: String(url), init });
        return Response.json(snapshot);
      },
    });
    const controller = new AbortController();
    expect(await client.chatgpt.models({ signal: controller.signal })).toEqual(
      snapshot,
    );
    expect(
      await client.chatgpt.refreshModels({ signal: controller.signal }),
    ).toEqual(snapshot);
    expect(
      calls.map(({ url, init }) => [url, init?.method ?? 'GET', init?.body]),
    ).toEqual([
      ['/api/providers/chatgpt/models', 'GET', undefined],
      ['/api/providers/chatgpt/models/refresh', 'POST', '{}'],
    ]);
    expect(calls.every(({ init }) => init?.signal === controller.signal)).toBe(
      true,
    );
    expect(
      calls.every(
        ({ init }) =>
          (init?.headers as Record<string, string>)['cache-control'] ===
          'no-store',
      ),
    ).toBe(true);
    expect(calls[1].init?.headers).toMatchObject({
      'content-type': 'application/json',
    });
  });

  it('m49_chatgpt_models_surfaces_busy_and_vault_errors_without_retry', async () => {
    for (const status of [409, 503]) {
      let requests = 0;
      const client = createDaemonClient({
        fetch: async () => {
          requests += 1;
          return Response.json(
            { error: 'Model metadata is unavailable.' },
            { status },
          );
        },
      });
      await expect(client.chatgpt.refreshModels()).rejects.toMatchObject({
        status,
        message: 'Model metadata is unavailable.',
      });
      expect(requests).toBe(1);
    }
  });

  it('m49_chatgpt_models_passes_cancellation_and_never_replays_a_dropped_refresh', async () => {
    const controller = new AbortController();
    let requests = 0;
    const client = createDaemonClient({
      fetch: async (_url, init) => {
        requests += 1;
        expect(init?.signal).toBe(controller.signal);
        return new Promise<Response>((_resolve, reject) => {
          init?.signal?.addEventListener(
            'abort',
            () => reject(new DOMException('Aborted', 'AbortError')),
            { once: true },
          );
        });
      },
    });
    const refresh = client.chatgpt.refreshModels({ signal: controller.signal });
    controller.abort();
    await expect(refresh).rejects.toBeDefined();
    expect(requests).toBe(1);
  });

  it('uses the redacted lifecycle endpoints without caching or credentials in bodies', async () => {
    const calls: { url: string; init?: RequestInit }[] = [];
    const status: ChatGptStatus = {
      connected: false,
      accountId: null,
      planType: null,
      login: null,
      error: null,
    };
    const client = createDaemonClient({
      baseUrl: '',
      fetch: async (url, init) => {
        calls.push({ url: String(url), init });
        return Response.json(status);
      },
    });
    for (const action of ['status', 'login', 'cancel', 'disconnect'] as const) {
      expect(await client.chatgpt[action]()).toEqual(status);
    }
    expect(
      calls.map(({ url, init }) => [
        url,
        init?.method ?? 'GET',
        (init?.headers as Record<string, string>)['cache-control'],
        init?.body,
      ]),
    ).toEqual([
      ['/api/providers/chatgpt/status', 'GET', 'no-store', undefined],
      ['/api/providers/chatgpt/login', 'POST', 'no-store', undefined],
      ['/api/providers/chatgpt/login', 'DELETE', 'no-store', undefined],
      ['/api/providers/chatgpt', 'DELETE', 'no-store', undefined],
    ]);
  });

  it('surfaces host failures', async () => {
    const client = createDaemonClient({
      fetch: async () =>
        Response.json({ error: 'vault_unavailable' }, { status: 503 }),
    });
    await expect(client.chatgpt.disconnect()).rejects.toBeInstanceOf(
      DaemonHttpError,
    );
  });
});

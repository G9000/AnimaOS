import {
  expect,
  type Page,
  type Request as PlaywrightRequest,
  type Route,
} from '@playwright/test';
import { createHash } from 'node:crypto';
import type {
  AgentEvent,
  AgentFilesPreview,
  ChatModelPreferences,
  ChatModelPreferencesView,
  ChatResult,
  ChatReview,
  DiscordConnector,
  LearningPolicy,
  LearningReview,
  Run,
  SessionMessage,
  SnapshotRun,
} from '@animaos/sdk';
import { AGENT, ORIGIN, createState, queuedRun, session } from './state';
import { staticAsset } from './static';
import { disconnect, emit, installStreams } from './stream';
import {
  createWorkspaceFilesPolicy,
  type WorkspaceFilesStep,
} from './workspace-files-policy';
export interface FixtureCall {
  method: string;
  path: string;
  body: unknown;
  key: string | null;
  headers: Record<string, string>;
}
export interface ConsoleFixtureOptions {
  origin?: string;
  realStatic?: boolean;
  realMemory?: boolean;
  realWorkspaceFiles?: boolean;
  chatSuite?: {
    ownerApiKey?: string;
    results?: ChatResult[];
    viewer?: { shareId: string; viewerId: string; secret: string; revoked?: boolean };
  };
}
export interface RealMemoryResponse {
  method: string;
  path: string;
  status: number;
  body: unknown;
}
export interface RealWorkspaceFilesResponse {
  kind: WorkspaceFilesStep;
  path: string;
  status: number;
  bytes: number;
  redirects: number;
  sha256: string;
  body: unknown;
}
async function installWorkspaceFilesRedirectGuard(page: Page) {
  await page.addInitScript(() => {
    const previousFetch = globalThis.fetch.bind(globalThis);
    globalThis.fetch = (input, init) => {
      const url = new URL(
        typeof input === 'string'
          ? input
          : input instanceof URL
            ? input.href
            : input.url,
        location.href,
      );
      return previousFetch(
        input,
        url.origin === 'http://127.0.0.1:18753' &&
          [
            '/api/workspace/files',
            '/api/workspace/file',
            '/api/workspace/file/download',
          ].includes(url.pathname)
          ? { ...init, redirect: 'manual' }
          : init,
      );
    };
  });
}
export async function installMemoryRedirectGuard(page: Page, origin: string) {
  await page.addInitScript((allowedOrigin) => {
    const previousFetch = globalThis.fetch.bind(globalThis);
    globalThis.fetch = (input, init) => {
      let url: URL;
      try {
        url = new URL(
          typeof input === 'string'
            ? input
            : input instanceof URL
              ? input.href
              : input.url,
          location.href,
        );
      } catch {
        return previousFetch(input, init);
      }
      return previousFetch(
        input,
        url.origin === allowedOrigin &&
          (url.pathname === '/api/memories' ||
            url.pathname.startsWith('/api/memories/'))
          ? { ...init, redirect: 'manual' }
          : init,
      );
    };
  }, origin);
}
export async function installConsoleFixture(
  page: Page,
  state = createState(),
  options: ConsoleFixtureOptions = {},
) {
  const models = new Map<string, ChatModelPreferencesView>();
  const reviews = new Map<string, ChatReview[]>();
  const results = options.chatSuite?.results ?? [];
  const modelView = (id: string): ChatModelPreferencesView => {
    let view = models.get(id);
    if (!view) {
      view = {
        preferences: { primary: { mode: 'inherit' }, reviewer: { mode: 'inherit' } },
        primary: { provider: 'openai', model: 'test-model' },
        reviewer: { provider: 'openai', model: 'review-model' },
        reviewerUnavailableReason: null, appliesTo: 'next_turn',
        clearGeneration: state.sessions.find(s => s.id === id)?.clearGeneration ?? 0,
        availableModels: ['test-model', 'alternate-model', 'review-model'].map(model => ({
          provider: 'openai', model, reasoningEfforts: ['low', 'medium', 'high'],
        })),
      };
      models.set(id, view);
    }
    return view;
  };
  const origin = options.origin ?? ORIGIN;
  if (
    (options.realStatic || options.realMemory || options.realWorkspaceFiles) &&
    origin !== 'http://127.0.0.1:18753'
  ) {
    throw new Error(
      'The real console fixture requires its isolated loopback origin',
    );
  }
  if (options.realWorkspaceFiles && (!options.realStatic || options.realMemory))
    throw new Error('Real workspace files require isolated static mode only');
  const calls: FixtureCall[] = [],
    unexpected: string[] = [],
    errors: string[] = [];
  const realResponses: RealMemoryResponse[] = [];
  const workspaceFilesPolicy = createWorkspaceFilesPolicy();
  const workspaceFileResponses: RealWorkspaceFilesResponse[] = [];
  const workspaceFileResponseCounts = {
    success: 0,
    redirects: 0,
    errors: 0,
    bytes: 0,
  };
  const pendingWorkspaceResponses = new Set<Promise<null>>();
  const workspaceAdmitted = new Map<
    PlaywrightRequest,
    {
      kind: WorkspaceFilesStep;
      path: string;
      done: Promise<null>;
      finish(failure?: string): void;
    }
  >();
  const admitWorkspace = (
    request: PlaywrightRequest,
    kind: WorkspaceFilesStep,
    path: string,
  ) => {
    let complete!: (value: null) => void;
    const done = new Promise<null>((resolve) => {
      complete = resolve;
    });
    const finish = (failure?: string) => {
      if (!workspaceAdmitted.delete(request)) return;
      clearTimeout(timer);
      if (failure) {
        workspaceFileResponseCounts.errors++;
        unexpected.push(`${failure}: GET ${path}`);
      }
      pendingWorkspaceResponses.delete(done);
      complete(null);
    };
    const timer = setTimeout(
      () => finish('Timed out real workspace response'),
      5000,
    );
    workspaceAdmitted.set(request, { kind, path, done, finish });
    pendingWorkspaceResponses.add(done);
  };
  page.on('requestfailed', (request) => {
    workspaceAdmitted.get(request)?.finish('Failed real workspace request');
  });
  page.on('response', (response) => {
    const entry = workspaceAdmitted.get(response.request());
    if (!entry) return;
    const status = response.status();
    const headers = response.headers();
    if (status >= 300 && status < 400) workspaceFileResponseCounts.redirects++;
    if (
      status !== 200 ||
      headers['location'] !== undefined ||
      response.request().redirectedFrom()
    ) {
      entry.finish('Refused real workspace status or redirect');
      return;
    }
    // These three known daemon responses have an explicit, uncompressed size.
    // Refuse before body() unless the header bounds the observation, then also
    // verify the received length. Never proxy or reconstruct the response.
    const length = headers['content-length'];
    const type = headers['content-type']?.split(';', 1)[0].trim().toLowerCase();
    if (
      !length ||
      !/^[0-9]+$/.test(length) ||
      Number(length) < 1 ||
      Number(length) > 262_144 ||
      headers['content-encoding'] !== undefined ||
      type !==
        (entry.kind === 'download'
          ? 'application/octet-stream'
          : 'application/json')
    ) {
      entry.finish('Refused real workspace response headers');
      return;
    }
    if (
      entry.kind === 'download' &&
      (headers['cache-control'] !== 'no-store' ||
        headers['x-content-type-options'] !== 'nosniff' ||
        headers['content-disposition'] !==
          'attachment; filename="ledger.csv"; filename*=UTF-8\'\'ledger.csv')
    ) {
      entry.finish('Refused report download headers');
      return;
    }
    void (async () => {
      const bytes = await Promise.race([response.body(), entry.done]);
      if (bytes === null) return;
      if (bytes.length !== Number(length) || bytes.length > 262_144)
        return entry.finish('Refused real workspace response size');
      const body: unknown =
        entry.kind === 'download' ? null : JSON.parse(bytes.toString('utf8'));
      workspaceFileResponses.push({
        kind: entry.kind,
        path: entry.path,
        status,
        bytes: bytes.length,
        redirects: 0,
        sha256: createHash('sha256').update(bytes).digest('hex'),
        body,
      });
      workspaceFileResponseCounts.success++;
      workspaceFileResponseCounts.bytes += bytes.length;
      entry.finish();
    })().catch(() => entry.finish('Failed real workspace response'));
  });
  const realCounts = new Map<string, number>();
  const realResponseCounts = { success: 0, redirects: 0, errors: 0, bytes: 0 };
  const pendingRealResponses = new Set<Promise<null>>();
  const admitted = new Map<
    PlaywrightRequest,
    {
      method: string;
      path: string;
      done: Promise<null>;
      finish(failure?: string): void;
    }
  >();
  const admit = (request: PlaywrightRequest, method: string, path: string) => {
    let resolve!: (value: null) => void;
    const done = new Promise<null>((complete) => {
      resolve = complete;
    });
    const finish = (failure?: string) => {
      if (!admitted.delete(request)) return;
      clearTimeout(timer);
      if (failure) {
        realResponseCounts.errors++;
        unexpected.push(`${failure}: ${method} ${path}`);
      }
      pendingRealResponses.delete(done);
      resolve(null);
    };
    const timer = setTimeout(
      () => finish('Timed out real memory response'),
      5000,
    );
    admitted.set(request, { method, path, done, finish });
    pendingRealResponses.add(done);
  };
  page.on('requestfailed', (request) => {
    admitted.get(request)?.finish('Failed real memory request');
  });
  page.on('response', (response) => {
    const entry = admitted.get(response.request());
    if (!entry) return;
    const status = response.status();
    if (status >= 200 && status < 300) realResponseCounts.success++;
    if (status >= 300 && status < 400) {
      realResponseCounts.redirects++;
      entry.finish('Refused real memory redirect');
      return;
    }
    void (async () => {
      if (Number(response.headers()['content-length']) > 262_144)
        return entry.finish('Oversized real memory response');
      const bytes = await Promise.race([response.body(), entry.done]);
      if (bytes === null) return;
      realResponseCounts.bytes += bytes.length;
      if (bytes.length > 262_144)
        return entry.finish('Oversized real memory response');
      let body: unknown;
      try {
        body = JSON.parse(bytes.toString('utf8'));
      } catch {
        return entry.finish('Non-JSON real memory response');
      }
      realResponses.push({
        method: entry.method,
        path: entry.path,
        status,
        body,
      });
      entry.finish(
        status < 200 || status >= 300
          ? 'Unsuccessful real memory response'
          : undefined,
      );
    })().catch(() => entry.finish('Failed real memory response'));
  });
  let realMemoryId: string | null = null;
  const expectedHttpErrors: string[] = [];
  const refusedUrls = new Set<string>();
  let connections = 0,
    seq = 1;
  const toolMessages = new Map<string, SessionMessage[]>();
  const persistentFailures = new Map<
    string,
    { status: number; body: unknown }
  >();
  const failures: {
    method: string;
    path: string;
    status: number;
    body: unknown;
  }[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  page.on('console', (message) => {
    if (!['error', 'warning'].includes(message.type())) return;
    if (
      message.type() === 'error' &&
      message
        .text()
        .startsWith(
          'Failed to load resource: the server responded with a status of ',
        ) &&
      refusedUrls.has(message.location().url)
    )
      expectedHttpErrors.push(message.text());
    else errors.push(message.text());
  });
  await installStreams(
    page,
    state,
    () => {
      connections++;
      seq = 1;
    },
    origin,
  );
  if (options.realMemory) await installMemoryRedirectGuard(page, origin);
  if (options.realWorkspaceFiles)
    await installWorkspaceFilesRedirectGuard(page);
  const json = (route: Route, body: unknown, status = 200) =>
    route.fulfill({
      status,
      contentType: 'application/json',
      body: JSON.stringify(body),
    });
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url());
    if (url.origin !== origin) {
      if (options.realWorkspaceFiles)
        workspaceFilesPolicy.admit({
          method: request.method(),
          url: request.url(),
        });
      unexpected.push(`External ${url.origin}${url.pathname}`);
      return route.abort('blockedbyclient');
    }
    if (options.realWorkspaceFiles && request.method() !== 'GET') {
      workspaceFilesPolicy.admit({
        method: request.method(),
        url: request.url(),
      });
      unexpected.push(
        `Refused browser mutation: ${request.method()} ${url.pathname}`,
      );
      return route.abort('blockedbyclient');
    }
    if (!url.pathname.startsWith('/api/')) {
      try {
        if (options.realStatic) {
          // The same contained built asset must exist locally before preview
          // is permitted to serve it; /health and unknown paths are refused.
          await staticAsset(url.pathname);
          return route.continue();
        }
        return await route.fulfill(await staticAsset(url.pathname));
      } catch (error) {
        unexpected.push(String(error));
        if (options.realStatic) return route.abort('blockedbyclient');
        return route.fulfill({ status: 404, body: 'Fixture asset missing' });
      }
    }
    let path: string;
    try {
      path = decodeURIComponent(url.pathname);
    } catch (error) {
      if (!options.realWorkspaceFiles) throw error;
      workspaceFilesPolicy.admit({
        method: request.method(),
        url: request.url(),
      });
      unexpected.push('Refused malformed workspace URL');
      return route.abort('blockedbyclient');
    }
    const method = request.method();
    let body: Record<string, unknown> = {};
    if (
      request.postData() &&
      request.headers()['content-type']?.includes('application/json')
    )
      body = request.postDataJSON();
    calls.push({
      method,
      path,
      body,
      key: request.headers()['idempotency-key'] ?? null,
      headers: request.headers(),
    });
    const viewer = options.chatSuite?.viewer;
    if (path.startsWith('/api/shared/conversations/')) {
      const headers = request.headers();
      if (!viewer || viewer.revoked || method !== 'GET' ||
        ![`/api/shared/conversations/${viewer.shareId}`, `/api/shared/conversations/${viewer.shareId}/messages`].includes(path) ||
        headers['x-anima-viewer-id'] !== viewer.viewerId || headers['x-anima-viewer-secret'] !== viewer.secret) {
        refusedUrls.add(request.url());
        return json(route, { error: 'Viewer access is unavailable.' }, 401);
      }
      if (path.endsWith('/messages')) return json(route, {
        title: 'Shared planning conversation', attachmentsOmitted: true, nextBefore: null,
        messages: [
          { id: 'shared-user', role: 'user', text: 'What should I prepare?', createdAtMs: 1 },
          { id: 'shared-answer', role: 'assistant', text: 'Bring the agenda and a notebook.', createdAtMs: 2 },
        ],
      });
      return json(route, { id: viewer.shareId, title: 'Shared planning conversation', viewerName: 'Guest reviewer', readOnly: true, attachmentsOmitted: true });
    }
    if (options.chatSuite?.ownerApiKey && request.headers()['x-api-key'] !== options.chatSuite.ownerApiKey) {
      refusedUrls.add(request.url());
      return json(route, { error: 'Owner API key required.' }, 401);
    }
    if (options.realWorkspaceFiles) {
      if (path.startsWith('/api/workspace/file')) {
        const admission = workspaceFilesPolicy.admit({
          method,
          url: request.url(),
        });
        if (!admission.allowed) {
          unexpected.push(admission.reason);
          return route.abort('blockedbyclient');
        }
        admitWorkspace(request, admission.kind, admission.path);
        try {
          return await route.continue();
        } catch {
          workspaceAdmitted
            .get(request)
            ?.finish('Failed real workspace continuation');
        }
        return;
      }
      // The existing, explicitly matched shell/bootstrap/provider GET mocks
      // below are synthetic. Mutations were refused before any continuation.
    }
    if (options.realMemory && path.startsWith('/api/memories')) {
      const key = `${method} ${path}`;
      const limits: Record<string, number> = {
        'POST /api/memories': 1,
        'GET /api/memories/recent': 16,
        'GET /api/memories/search': 2,
        'GET /api/memories/facts': 16,
        'GET /api/memories/entities': 16,
        'GET /api/memories/relationships': 16,
        'GET /api/memories/readiness': 2,
      };
      if (realMemoryId !== null) {
        limits[`PATCH /api/memories/${realMemoryId}`] = 1;
        limits[`DELETE /api/memories/${realMemoryId}`] = 1;
      }
      const next = (realCounts.get(key) ?? 0) + 1;
      if (limits[key] === undefined || next > limits[key]) {
        unexpected.push(`Refused real memory request: ${key}`);
        return route.abort('blockedbyclient');
      }
      realCounts.set(key, next);
      admit(request, method, path);
      try {
        return await route.continue();
      } catch {
        admitted.get(request)?.finish('Failed real memory request');
      }
      return;
    }
    const persistent = persistentFailures.get(`${method} ${path}`);
    if (persistent) {
      refusedUrls.add(request.url());
      return json(route, persistent.body, persistent.status);
    }
    const fail = failures.findIndex(
      (f) => f.path === path && f.method === method,
    );
    if (fail >= 0) {
      const f = failures.splice(fail, 1)[0];
      refusedUrls.add(request.url());
      return json(route, f.body, f.status);
    }
    const get = method === 'GET';
    if (get && path === '/api/capabilities') return json(route, {});
    if (options.chatSuite && get && path === '/api/skill-drafts')
      return json(route, { drafts: [] });
    if (options.chatSuite && get && path === `/api/agents/${AGENT}/workflows`)
      return json(route, { workflows: [] });
    if (options.chatSuite && get && /^\/api\/agents\/companion\/sessions\/[^/]+\/clarifications$/.test(path))
      return json(route, { clarifications: [] });
    if (options.chatSuite && method === 'POST' && path === `/api/agents/${AGENT}/main-chat`) {
      let record = state.sessions.find(s => s.mainChat);
      if (!record) {
        record = { ...session(), mainChat: true, clearGeneration: 0 };
        state.sessions.push(record); state.messages[record.id] = [];
      }
      return json(route, { session: record });
    }
    const extension = options.chatSuite && path.match(/^\/api\/agents\/companion\/sessions\/([^/]+)\/(model-preferences|capabilities|reviews|results|shares)(?:\/([^/]+))?$/);
    if (extension) {
      const [, id, resource, child] = extension;
      const generation = state.sessions.find(s => s.id === id)?.clearGeneration ?? 0;
      if (resource === 'model-preferences') {
        const view = modelView(id);
        if (method === 'PATCH') {
          for (const target of ['primary', 'reviewer'] as const) {
            const preference = body[target] as ChatModelPreferences[typeof target] | undefined;
            if (preference) {
              view.preferences[target] = preference;
              view[target] = preference.mode === 'override' ? {
                provider: preference.provider, model: preference.model,
                ...(preference.reasoningEffort ? { reasoningEffort: preference.reasoningEffort } : {}),
              } : { provider: 'openai', model: target === 'primary' ? 'test-model' : 'review-model' };
            }
          }
        }
        if (get || method === 'PATCH') return json(route, view);
      }
      if (get && resource === 'capabilities') return json(route, {
        agentId: AGENT, sessionId: id, basis: 'next_owner_turn', runId: null, preparing: false,
        notice: 'Approval policy still applies to every tool call.', connections: [],
        tools: [{ name: 'read_file', description: 'Read an allowed workspace file', origin: 'native', risk: 'read', approval: 'allow', argumentDependent: true, available: true, reason: null }],
      });
      if (resource === 'reviews') {
        if (get) return json(route, { reviews: reviews.get(id) ?? [] });
        if (method === 'POST') {
          const row: ChatReview = {
            id: 'review-fixture', requestId: String(body.requestId), sourceMessageId: 'answer-1', sourceRunId: 'run-answer',
            evidenceDigest: '0'.repeat(64), focus: typeof body.focus === 'string' ? body.focus : null,
            reviewer: modelView(id).reviewer, status: 'completed', createdAtMs: 3, finishedAtMs: 4,
            findings: 'The answer is consistent with the supplied chat evidence. Repository and live state were not inspected.',
            error: null, evidenceLimited: true,
          };
          reviews.set(id, [row]); return json(route, { review: row }, 202);
        }
      }
      if (resource === 'results') {
        if (get) return json(route, { clearGeneration: generation, results: results.filter(result => result.sessionId === id) });
        if (method === 'PATCH' && child) {
          const result = results.find(result => result.id === child && result.sessionId === id);
          if (!result) return json(route, { error: 'Unknown result' }, 404);
          expect(body).toMatchObject({ clearGeneration: generation, expectedRevision: result.revision });
          result.pinned = body.pinned === true; result.revision += 1;
          return json(route, { result });
        }
      }
      if (resource === 'shares') {
        if (get) return json(route, { grants: [] });
        if (method === 'POST' && viewer) return json(route, {
          grant: { id: viewer.shareId, viewerId: viewer.viewerId, viewerName: String(body.viewerName), createdAtMs: 5, revokedAtMs: null, active: true }, secret: viewer.secret,
        }, 201);
      }
    }
    if (get && path === '/api/health') return json(route, { status: 'ok' });
    if (get && path === `/api/agents/${AGENT}/connectors/discord`) {
      const dormant: { connectors: DiscordConnector[] } = { connectors: [] };
      return json(route, dormant);
    }
    if (get && path === `/api/agents/${AGENT}/agent-files`) {
      const preview: AgentFilesPreview = {
        mode: 'current_preview',
        availability: 'unavailable_workspace',
        capturedAtMs: 1,
        snapshotDigest: '0'.repeat(64),
        identitySource: 'legacy',
        projectSourceKind: null,
        estimatedTokens: 0,
        entries: [],
      };
      return json(route, preview);
    }
    if (get && path === `/api/agents/${AGENT}/learning-policy`) {
      const policy: LearningPolicy = {
        autoEnabled: false,
        dailyLimit: 2,
        pendingLimit: 10,
        timeoutMs: 20_000,
        physicalReviewerActive: false,
      };
      return json(route, policy);
    }
    if (get && path === `/api/agents/${AGENT}/learning-reviews`) {
      const dormant: { reviews: LearningReview[] } = { reviews: [] };
      return json(route, dormant);
    }
    if (get && path === '/api/status') return json(route, state.status);
    if (get && path === '/api/providers')
      return json(route, {
        providers: [
          {
            id: 'openai',
            label: 'OpenAI',
            configured: true,
            requiresKey: true,
            apiKeyEnvs: ['OPENAI_API_KEY'],
          },
        ],
      });
    if (get && path === '/api/workspace')
      return json(route, {
        configured: state.configured,
        workspace: state.configured
          ? {
              rootPath: '/workspace',
              companyName: 'Personal',
              mission: 'Everyday life',
              values: [],
              hasAvatar: false,
            }
          : null,
        defaultRoot: '/workspace',
      });
    if (method === 'POST' && path === '/api/workspace/bootstrap') {
      expect(body).not.toHaveProperty('workers');
      state.agents = [createState().agents[0]];
      state.configured = true;
      return json(route, { agent: state.agents[0] });
    }
    if (get && path === '/api/agents')
      return json(route, {
        agents:
          url.searchParams.get('view') === 'summary'
            ? state.agents.map(({ messages: _messages, ...summary }) => summary)
            : state.agents,
      });
    if (get && path === '/api/skills') return json(route, { skills: [] });
    if (get && path === '/api/approvals')
      return json(route, {
        approvals: state.approvals.filter((a) => a.status === 'pending'),
        nextCursor: null,
      });
    if (get && path === `/api/agents/${AGENT}/avatar`)
      return route.fulfill({
        contentType: 'image/png',
        body: Buffer.from(
          'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a9uoAAAAASUVORK5CYII=',
          'base64',
        ),
      });
    if (get && path === `/api/agents/${AGENT}/connectors`)
      return json(route, { connectors: [] });
    if (get && path === `/api/agents/${AGENT}/schedules`)
      return json(route, { schedules: [] });
    if (path === `/api/agents/${AGENT}/sessions`) {
      if (get)
        return json(route, { sessions: state.sessions, nextCursor: null });
      if (method === 'POST') {
        expect(Object.keys(body)).toEqual([]);
        const created = session();
        state.sessions.push(created);
        state.messages[created.id] = [];
        return json(route, { session: created }, 201);
      }
    }
    const scoped = path.match(
      /^\/api\/agents\/companion\/sessions\/([^/]+)(?:\/(messages|runs|attachments))?$/,
    );
    if (scoped) {
      const id = scoped[1],
        record = state.sessions.find((s) => s.id === id);
      if (!record) throw new Error(`Unknown session ${id}`);
      if (get && scoped[2] === 'messages')
        return json(route, {
          messages: state.messages[id] ?? [],
          nextBefore: null,
        });
      if (get && scoped[2] === 'runs')
        return json(route, {
          runs: state.runs.filter((r) => r.sessionId === id).reverse(),
        });
      if (get && !scoped[2]) return json(route, { session: record });
      if (method === 'PATCH' && !scoped[2]) {
        expect(
          Object.keys(body).every((k) =>
            ['lastReadAtMs', 'title', 'archived'].includes(k),
          ),
        ).toBe(true);
        Object.assign(record, body);
        return json(route, { session: record });
      }
      if (method === 'POST' && scoped[2] === 'runs') {
        expect(request.headers()['idempotency-key']).toBeTruthy();
        expect(typeof body.text).toBe('string');
        expect(['queue', 'steer']).toContain(body.mode);
        expect(
          Object.keys(body).every((k) =>
            ['text', 'attachmentIds', 'skill', 'mode'].includes(k),
          ),
        ).toBe(true);
        const attachments = (body.attachmentIds ?? []) as string[];
        expect(
          attachments.every((id) => state.attachments.some((a) => a.id === id)),
        ).toBe(true);
        const run = queuedRun(
          id,
          {
            text: body.text as string,
            attachmentIds: attachments,
            skill: (body.skill as string) ?? null,
          },
          request.headers()['idempotency-key'],
        );
        state.runs.push(run);
        state.snapshotRuns = [
          { run, stepId: null, text: '', textOffset: 0, tools: [] },
        ];
        record.activeRuns = 1;
        if (run.input.text) record.title = run.input.text;
        return json(route, { run }, 202);
      }
      if (method === 'POST' && scoped[2] === 'attachments') {
        expect(request.headers()['content-type']).toContain(
          'multipart/form-data',
        );
        const attachment = {
          id: 'att_1',
          name: 'pixel.png',
          mime: 'image/png',
          size: 68,
          kind: 'image' as const,
          path: 'attachments/pixel.png',
          createdAtMs: 1,
        };
        state.attachments.push(attachment);
        return json(route, { attachment }, 201);
      }
    }
    const download = path.match(
      /^\/api\/agents\/companion\/attachments\/(att_1)$/,
    );
    if (get && download && state.attachments.some((a) => a.id === download[1]))
      return route.fulfill({
        contentType: 'image/png',
        body: Buffer.from(
          'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a9uoAAAAASUVORK5CYII=',
          'base64',
        ),
      });
    const runPath = path.match(
      /^\/api\/agents\/companion\/runs\/([^/]+)(?:\/(stop))?$/,
    );
    if (runPath) {
      const run = state.runs.find((r) => r.id === runPath[1]);
      if (!run) throw new Error('Unknown run');
      if (get && !runPath[2]) return json(route, { run });
      if (method === 'POST' && runPath[2] === 'stop') {
        Object.assign(run, {
          status: 'cancelled',
          stop: { requestedAtMs: 20 },
          finishedAtMs: 20,
        });
        state.sessions.find((s) => s.id === run.sessionId)!.activeRuns = 0;
        state.snapshotRuns = [];
        return json(route, { run });
      }
    }
    const decision = path.match(/^\/api\/approvals\/([^/]+)\/decision$/);
    if (method === 'POST' && decision) {
      const approval = state.approvals.find((a) => a.id === decision[1]);
      if (!approval || approval.status !== 'pending')
        throw new Error('Invalid approval');
      expect(body.revision).toBe(approval.revision);
      expect(['allow_once', 'deny']).toContain(body.decision);
      approval.status = body.decision === 'deny' ? 'denied' : 'allowed';
      approval.revision++;
      return json(route, { approval });
    }
    unexpected.push(`${method} ${path}`);
    return json(
      route,
      { error: `UNIMPLEMENTED FIXTURE: ${method} ${path}` },
      501,
    );
  });
  return {
    state,
    calls,
    realResponses,
    realCounts,
    realResponseCounts,
    workspaceFilesPolicy,
    workspaceFileResponses,
    workspaceFileResponseCounts,
    async waitForWorkspaceResponses() {
      await Promise.all([...pendingWorkspaceResponses]);
    },
    async waitForRealResponses() {
      // Every admitted request owns a five-second timer, including no-response failures.
      await Promise.all([...pendingRealResponses]);
    },
    setRealMemoryId(id: string) {
      if (
        !options.realMemory ||
        realMemoryId !== null ||
        !/^[A-Za-z0-9_-]{1,100}$/.test(id)
      )
        throw new Error('Invalid or repeated real memory ID');
      realMemoryId = id;
    },
    expectedHttpErrors,
    unexpected,
    connections: () => connections,
    async waitForStream() {
      await expect.poll(() => connections).toBeGreaterThan(0);
      await expect
        .poll(() =>
          page.evaluate(
            () =>
              (
                globalThis as unknown as { consoleStreams: { count(): number } }
              ).consoleStreams?.count() ?? 0,
          ),
        )
        .toBe(1);
    },
    async emit(event: AgentEvent) {
      if (event.type === 'tool.started' || event.type === 'tool.finished') {
        const messages = toolMessages.get(event.runId!) ?? [];
        if (event.type === 'tool.started')
          messages.push({
            id: 'tool_call_' + event.toolCallId,
            role: 'assistant',
            text: '',
            attachments: [],
            metadata: {
              runId: event.runId,
              stepId: event.stepId,
              toolCalls: [
                {
                  id: event.toolCallId,
                  name: event.name,
                  args: JSON.parse(event.argumentsPreview),
                },
              ],
            },
            createdAtMs: 14,
          });
        else
          messages.push({
            id: 'tool_result_' + event.toolCallId,
            role: 'tool',
            text: event.resultPreview,
            attachments: [],
            metadata: {
              runId: event.runId,
              stepId: event.stepId,
              toolCallId: event.toolCallId,
              toolStatus: event.status,
              toolDurationMs: event.durationMs,
            },
            createdAtMs: 16,
          });
        toolMessages.set(event.runId!, messages);
      }
      const framed = { ...event, seq: ++seq } as AgentEvent;
      await emit(page, framed);
    },
    disconnectStream: () => disconnect(page),
    setSnapshot(runs: SnapshotRun[]) {
      state.snapshotRuns = runs;
    },
    setRouteFailure(method: string, path: string, status: number | null) {
      if (status === null) persistentFailures.delete(`${method} ${path}`);
      else
        persistentFailures.set(`${method} ${path}`, {
          status,
          body: { error: 'fixture refusal' },
        });
    },
    failNext(
      method: string,
      path: string,
      status: number,
      body: unknown = { error: 'fixture refusal' },
    ) {
      failures.push({ method, path, status, body });
    },
    start(run: Run) {
      run.status = 'running';
      run.startedAtMs = 12;
    },
    requestApproval(approval: import('@animaos/sdk').Approval) {
      state.approvals.push(approval);
      state.runs.find((r) => r.id === approval.runId)!.status =
        'awaiting_approval';
    },
    persistPartial(run: Run, text: string) {
      state.messages[run.sessionId].push({
        id: 'partial_1',
        role: 'assistant',
        text,
        attachments: [],
        metadata: { runId: run.id, stopped: true },
        createdAtMs: 20,
      });
    },
    finish(run: Run, text: string) {
      Object.assign(run, {
        status: 'completed',
        finishedAtMs: 30,
        replyMessageId: 'reply_1',
      });
      state.snapshotRuns = [];
      state.sessions.find((s) => s.id === run.sessionId)!.activeRuns = 0;
      state.messages[run.sessionId].push({
        id: 'user_1',
        role: 'user',
        text: run.input.text,
        attachments: [],
        metadata: { runId: run.id },
        createdAtMs: 10,
      });
      state.messages[run.sessionId].push(...(toolMessages.get(run.id) ?? []));
      state.messages[run.sessionId].push({
        id: 'reply_1',
        role: 'assistant',
        text,
        attachments: [],
        metadata: { runId: run.id },
        createdAtMs: 30,
      });
    },
    assertClean() {
      expect(unexpected, 'Unexpected fixture requests').toEqual([]);
      expect(errors, 'Browser errors/warnings').toEqual([]);
    },
  };
}
export type ConsoleFixtureController = Awaited<
  ReturnType<typeof installConsoleFixture>
>;

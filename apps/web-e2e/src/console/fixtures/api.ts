import { expect, type Page, type Route } from '@playwright/test';
import type {
  AgentEvent,
  Run,
  SessionMessage,
  SnapshotRun,
} from '@animaOS-SWARM/sdk';
import { AGENT, ORIGIN, createState, queuedRun, session } from './state';
import { staticAsset } from './static';
import { disconnect, emit, installStreams } from './stream';
export interface FixtureCall {
  method: string;
  path: string;
  body: unknown;
  key: string | null;
}
export async function installConsoleFixture(page: Page, state = createState()) {
  const calls: FixtureCall[] = [],
    unexpected: string[] = [],
    errors: string[] = [];
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
  await installStreams(page, state, () => {
    connections++;
    seq = 1;
  });
  const json = (route: Route, body: unknown, status = 200) =>
    route.fulfill({
      status,
      contentType: 'application/json',
      body: JSON.stringify(body),
    });
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url());
    if (url.origin !== ORIGIN) {
      unexpected.push(`External ${url.origin}${url.pathname}`);
      return route.abort('blockedbyclient');
    }
    if (!url.pathname.startsWith('/api/')) {
      try {
        return await route.fulfill(await staticAsset(url.pathname));
      } catch (error) {
        unexpected.push(String(error));
        return route.fulfill({ status: 404, body: 'Fixture asset missing' });
      }
    }
    const path = decodeURIComponent(url.pathname),
      method = request.method();
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
    });
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
    if (get && path === '/api/health') return json(route, { status: 'ok' });
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
    requestApproval(approval: import('@animaOS-SWARM/sdk').Approval) {
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

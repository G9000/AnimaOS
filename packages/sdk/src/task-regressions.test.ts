import { describe, expect, it, vi } from 'vitest';
import { createDaemonClient, DaemonHttpError } from './client.js';

describe('task regressions owner transport', () => {
  it('stays idle until requested, unwraps receipts, and preserves exact owner-selected pins', async () => {
    const suite = {
      name: 'File task',
      fixtures: [
        {
          id: 'file',
          actor: {
            prompt: 'Write done to result.txt, then answer Done.',
            initialFiles: {},
            readableFiles: [],
            writableFiles: ['result.txt'],
            mockEffects: false,
          },
          oracle: {
            expectedFiles: { 'result.txt': 'done' },
            expectedOutput: 'Done.',
            expectedMockEffects: [],
          },
        },
      ],
    };
    const summary = {
      id: 'suite/a',
      digest: 'suite-pin',
      name: suite.name,
      fixtureCount: 1,
      createdAtMs: 1,
    };
    const configuration = {
      agentId: 'agent/a',
      configDigest: 'config-pin',
      provider: 'test',
      model: 'baseline',
      system: null,
    };
    const report = { id: 'run/a', status: 'running', usageComplete: false };
    const responses = [
      { suites: [summary] },
      { suite: summary },
      { configuration },
      { report },
      { report },
      { report: { ...report, status: 'cancelled' } },
    ];
    const fetch = vi.fn<typeof globalThis.fetch>(async () =>
      Response.json(responses.shift()),
    );
    const client = createDaemonClient({
      baseUrl: '',
      ownerToken: 'owner-only',
      fetch,
    });
    expect(fetch).not.toHaveBeenCalled();
    expect(client.taskRegressions).toBeDefined();
    const signal = new AbortController().signal;
    expect(await client.taskRegressions.suites({ signal })).toEqual([summary]);
    expect(await client.taskRegressions.define(suite)).toEqual(summary);
    expect(await client.taskRegressions.configuration('agent/a')).toEqual(
      configuration,
    );
    const input = {
      suiteId: summary.id,
      suiteDigest: summary.digest,
      baseline: { agentId: 'agent/a', configDigest: 'config-pin' },
      candidate: {
        agentId: 'agent/a',
        configDigest: 'config-pin',
        provider: 'test',
        model: 'candidate',
        system: '',
        toolPolicy: 'files' as const,
      },
      repetitions: 2,
      requestKey: 'owner-action-key',
    };
    expect(await client.taskRegressions.compare(input)).toEqual(report);
    expect(await client.taskRegressions.report('run/a')).toEqual(report);
    expect((await client.taskRegressions.cancel('run/a')).status).toBe(
      'cancelled',
    );
    expect(fetch.mock.calls.map(([url]) => String(url))).toEqual([
      '/api/task-regression-suites',
      '/api/task-regression-suites',
      '/api/task-regression-configs/agent%2Fa',
      '/api/task-regressions',
      '/api/task-regressions/run%2Fa',
      '/api/task-regressions/run%2Fa/cancel',
    ]);
    const inits = fetch.mock.calls.map(([, init]) => init!);
    expect(inits[0].signal).toBe(signal);
    expect(inits.map((init) => init.method ?? 'GET')).toEqual([
      'GET',
      'POST',
      'GET',
      'POST',
      'GET',
      'POST',
    ]);
    expect(JSON.parse(inits[1].body as string)).toEqual(suite);
    expect(JSON.parse(inits[3].body as string)).toEqual(input);
    expect(JSON.parse(inits[5].body as string)).toEqual({});
    for (const init of inits) {
      expect(init.cache).toBe('no-store');
      expect(new Headers(init.headers).get('authorization')).toBe(
        'Bearer owner-only',
      );
    }
  });

  it('surfaces stale admission without retrying a model-start action', async () => {
    const fetch = vi.fn<typeof globalThis.fetch>(async () =>
      Response.json({ error: 'configuration changed' }, { status: 409 }),
    );
    const client = createDaemonClient({ baseUrl: '', fetch });
    await expect(
      client.taskRegressions.compare({
        suiteId: 'suite',
        suiteDigest: 'pin',
        repetitions: 1,
        requestKey: 'same-key',
        baseline: { agentId: 'agent', configDigest: 'old' },
        candidate: { agentId: 'agent', configDigest: 'old', model: 'next' },
      }),
    ).rejects.toBeInstanceOf(DaemonHttpError);
    expect(fetch).toHaveBeenCalledTimes(1);
  });
});

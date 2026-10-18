import { describe, expect, expectTypeOf, it } from 'vitest';
import type { Run, StartRunInput } from './runs.js';

import {
  createDaemonClient,
  DaemonHttpError,
  isTerminalRunStatus,
} from './index.js';

function transport(respond: (url: string, init?: RequestInit) => Response) {
  const requests: { url: string; init?: RequestInit }[] = [];
  const client = createDaemonClient({
    baseUrl: '',
    fetch: async (url, init) => {
      requests.push({ url: String(url), init });
      return respond(String(url), init);
    },
  });
  return { runs: client.runs, requests };
}

const run = {
  id: 'run_1',
  agentId: 'agent/a',
  sessionId: 'chat:1',
  source: 'web',
  idempotencyKey: 'key-1',
  status: 'queued',
};

describe('runs client', () => {
  it('preserves optional async summaries and completion origins through owner reads', async () => {
    const asyncHelper = { ownerAgentId: 'owner', ownerSessionId: 'chat:owner', parentRunId: 'launch', completion: { state: 'awaiting_parent', reservedRunId: 'completion' }, pendingSteerCount: 1, archiveUnconsumedCount: 2 };
    const helperCompletion = { helperRunId: 'child', launchParentRunId: 'launch' };
    const { runs } = transport((url) => Response.json(url.includes('?') ? { runs: [{ ...run, asyncHelper }, { ...run, id: 'completion', helperCompletion }] } : { run }));
    const listed = await runs.listForSession('owner', 'chat:owner', { limit: 2 });
    expect(listed).toEqual([{ ...run, asyncHelper }, { ...run, id: 'completion', helperCompletion }]);
    expect(await runs.get('owner', 'legacy')).not.toHaveProperty('asyncHelper');
    expect(await runs.get('owner', 'legacy')).not.toHaveProperty('helperCompletion');
    expectTypeOf<Extract<keyof StartRunInput, 'asyncHelper' | 'helperCompletion' | 'parentRunId' | 'ownerAgentId'>>().toEqualTypeOf<never>();
    // Legacy payloads remain assignable after new response-only fields are added.
    expectTypeOf<Pick<Run, 'id' | 'source'>>().toEqualTypeOf<{ id: string; source: Run['source'] }>();
  });
  it('retains ordered attachment IDs unchanged in accepted run input', async () => {
    const { runs, requests } = transport(() =>
      Response.json({ run }, { status: 202 }),
    );
    const attachmentIds = ['att_second', 'att_first'];
    await runs.start(
      'agent/a',
      'chat:1',
      { text: '', attachmentIds, mode: 'steer', skill: 'writer' },
      { idempotencyKey: 'attachment-run' },
    );
    expect(JSON.parse(String(requests[0].init?.body))).toEqual({
      text: '',
      attachmentIds: ['att_second', 'att_first'],
      mode: 'steer',
      skill: 'writer',
    });
    expect(attachmentIds).toEqual(['att_second', 'att_first']);
  });
  it('starts a run with its idempotency key and returns the accepted run', async () => {
    const { runs, requests } = transport(() =>
      Response.json({ run }, { status: 202 }),
    );

    const accepted = await runs.start(
      'agent/a',
      'chat:1',
      { text: 'Plan the week', mode: 'queue' },
      { idempotencyKey: 'key-1' },
    );
    expect(accepted).toEqual({ run });
    expect(accepted.run.idempotencyKey).toBe('key-1');

    const [request] = requests;
    expect(request.url).toBe('/api/agents/agent%2Fa/sessions/chat%3A1/runs');
    expect(request.init?.method).toBe('POST');
    expect(
      (request.init?.headers as Record<string, string>)['idempotency-key'],
    ).toBe('key-1');
    expect(JSON.parse(String(request.init?.body))).toEqual({
      text: 'Plan the week',
      mode: 'queue',
    });
  });

  it('returns a steer that joined the active run', async () => {
    const { runs } = transport(() =>
      Response.json(
        { run: { ...run, status: 'running' }, steer: { status: 'pending' } },
        { status: 202 },
      ),
    );

    const result = await runs.start(
      'agent/a',
      'chat:1',
      { text: 'also this', mode: 'steer' },
      { idempotencyKey: 'key-2' },
    );

    expect(result.steer).toEqual({ status: 'pending' });
    expect(result.run.status).toBe('running');
  });

  it('stops, reads, and lists runs', async () => {
    const { runs, requests } = transport((url) =>
      url.endsWith('/runs?limit=5')
        ? Response.json({ runs: [run] })
        : url.endsWith('/stop')
          ? Response.json(
              { run: { ...run, status: 'cancelled' } },
              { status: 202 },
            )
          : Response.json({ run }),
    );

    expect((await runs.stop('agent/a', 'run_1')).status).toBe('cancelled');
    expect(await runs.get('agent/a', 'run_1')).toEqual(run);
    expect(
      await runs.listForSession('agent/a', 'chat:1', { limit: 5 }),
    ).toEqual([run]);
    expect(
      requests.map(({ url, init }) => [init?.method ?? 'GET', url]),
    ).toEqual([
      ['POST', '/api/agents/agent%2Fa/runs/run_1/stop'],
      ['GET', '/api/agents/agent%2Fa/runs/run_1'],
      ['GET', '/api/agents/agent%2Fa/sessions/chat%3A1/runs?limit=5'],
    ]);
  });

  it('surfaces a full queue as a daemon error', async () => {
    const { runs } = transport(() =>
      Response.json(
        {
          error:
            'This companion already has 8 queued messages; wait for one to start',
        },
        { status: 429 },
      ),
    );

    const failure = runs.start(
      'agent/a',
      'chat:1',
      { text: 'hi' },
      { idempotencyKey: 'k' },
    );
    await expect(failure).rejects.toBeInstanceOf(DaemonHttpError);
    await expect(failure).rejects.toMatchObject({ status: 429 });
  });

  it('tells terminal statuses apart', () => {
    expect(isTerminalRunStatus('completed')).toBe(true);
    expect(isTerminalRunStatus('cancelled')).toBe(true);
    expect(isTerminalRunStatus('interrupted')).toBe(true);
    expect(isTerminalRunStatus('queued')).toBe(false);
    expect(isTerminalRunStatus('awaiting_approval')).toBe(false);
  });
});

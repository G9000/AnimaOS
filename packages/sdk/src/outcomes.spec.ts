import { expect, it } from 'vitest';
import { createDaemonClient, DaemonHttpError } from './index.js';
import type { OutcomeDefinition, RunOutcome } from './outcomes.js';

const definition: OutcomeDefinition = {
  criteria: [{ kind: 'response_text', id: 'answer', contains: 'Ready' }],
  maxCorrections: 2,
  acceptance: 'owner_required',
};
const outcome: RunOutcome = {
  definition, revision: 4, rootRunId: 'run/one',
  taskSha256: 'a'.repeat(64), sourceSha256: 'b'.repeat(64),
  modelSha256: 'c'.repeat(64), workspaceSha256: 'd'.repeat(64),
  iterations: [], consumedCorrections: 1, status: 'awaiting_owner',
  reportSha256: 'e'.repeat(64), repairPaths: [],
};

it('keeps ordinary admission unchanged and transmits an explicit outcome with the same request key', async () => {
  const requests: { url: string; init?: RequestInit }[] = [];
  const client = createDaemonClient({ baseUrl: '', fetch: async (url, init) => {
    requests.push({ url: String(url), init });
    return Response.json({ run: { id: 'r', ...(requests.length === 2 ? { outcome } : {}) } });
  } });
  await client.runs.start('agent/a', 'chat/b', { text: 'Hello' }, { idempotencyKey: 'ordinary' });
  const result = await client.runs.start('agent/a', 'chat/b', { text: 'Make it ready', outcome: definition }, { idempotencyKey: 'explicit' });
  expect(JSON.parse(requests[0].init?.body as string)).toEqual({ text: 'Hello' });
  expect(JSON.parse(requests[1].init?.body as string)).toEqual({ text: 'Make it ready', outcome: definition });
  expect(new Headers(requests[1].init?.headers).get('idempotency-key')).toBe('explicit');
  expect(requests[1].url).toBe('/api/agents/agent%2Fa/sessions/chat%2Fb/runs');
  expect(result.run.outcome).toEqual(outcome);
});

it('reads and accepts the exact current report under owner authentication', async () => {
  const requests: { url: string; init?: RequestInit }[] = [];
  let conflict = false;
  const client = createDaemonClient({ baseUrl: '', ownerToken: 'owner-secret', fetch: async (url, init) => {
    requests.push({ url: String(url), init });
    return conflict ? Response.json({ error: 'Report changed' }, { status: 409 }) : Response.json({ outcome });
  } });
  const signal = new AbortController().signal;
  expect(await client.outcomes.get('agent/a', 'run/one', { signal })).toEqual(outcome);
  expect(requests[0].init?.signal).toBe(signal);
  const acceptance = { revision: outcome.revision, reportSha256: outcome.reportSha256! };
  expect(await client.outcomes.accept('agent/a', 'run/one', acceptance)).toEqual(outcome);
  expect(requests.map(({ url }) => url)).toEqual([
    '/api/agents/agent%2Fa/runs/run%2Fone/outcome',
    '/api/agents/agent%2Fa/runs/run%2Fone/outcome/accept',
  ]);
  expect(JSON.parse(requests[1].init?.body as string)).toEqual(acceptance);
  expect(new Headers(requests[1].init?.headers).get('authorization')).toBe('Bearer owner-secret');
  conflict = true;
  await expect(client.outcomes.accept('agent/a', 'run/one', acceptance)).rejects.toBeInstanceOf(DaemonHttpError);
});

it('closes a retained task by exact revision without retrying its execution', async () => {
  const requests: {url:string;init?:RequestInit}[]=[];
  const closed={...outcome,status:'stopped' as const,revision:5};
  const client=createDaemonClient({baseUrl:'',ownerToken:'owner-secret',fetch:async(url,init)=>{
    requests.push({url:String(url),init});return Response.json({outcome:closed});
  }});
  expect(await client.outcomes.close('agent/a','run/one',{revision:4})).toEqual(closed);
  expect(requests).toHaveLength(1);
  expect(requests[0].url).toBe('/api/agents/agent%2Fa/runs/run%2Fone/outcome/close');
  expect(JSON.parse(requests[0].init?.body as string)).toEqual({revision:4});
});

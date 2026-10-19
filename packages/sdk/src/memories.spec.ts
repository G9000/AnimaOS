import { describe, expect, it } from 'vitest';

import {
  createDaemonClient,
  DaemonHttpError,
  MAX_FACT_VALUE_CHARS,
  MAX_FACTS_SHOWN,
  MAX_MEMORY_EDIT_CHARS,
  MAX_MEMORY_TAG_CHARS,
  MAX_MEMORY_TAGS,
  type MemoryFact,
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
  return { memories: client.memories, requests };
}

const fact: MemoryFact = {
  id: 'fact-1',
  subjectKind: 'user',
  subjectId: 'owner',
  subjectName: 'Owner',
  predicate: 'likes',
  objectKind: null,
  objectId: null,
  objectName: null,
  value: 'tea',
  validFrom: null,
  validTo: null,
  observedAt: 1,
  confidence: 1,
  evidenceMemoryIds: [],
  supersedesFactIds: [],
  status: 'active',
  tags: null,
  roomId: null,
  worldId: null,
  sessionId: null,
  createdAt: 1,
  updatedAt: 1,
};

describe('memories client: edits, facts, and entities', () => {
  it('reads, adds, edits and removes core memory entries on the core-memory routes', async () => {
    const block = (
      entries: { id: string; content: string }[],
      limit: number,
    ) => ({
      used: entries.reduce((sum, entry) => sum + entry.content.length, 0),
      limit,
      entries: entries.map((entry) => ({ ...entry, flagged: [] })),
    });
    const view = {
      user: block([{ id: 'mem-1', content: 'Short answers' }], 1500),
      notes: block([], 2500),
    };
    const edit = { ...view, entryId: 'mem-1', changed: true, warnings: [] };
    const { memories, requests } = transport((url) =>
      url.endsWith('/core-memory') ? Response.json(view) : Response.json(edit),
    );

    expect(await memories.coreMemory('agent/a')).toEqual(view);
    expect(
      await memories.addCoreEntry('agent/a', 'user', 'Short answers'),
    ).toEqual(edit);
    expect(
      await memories.updateCoreEntry(
        'agent/a',
        'notes',
        'mem 1',
        'Keep it short',
      ),
    ).toEqual(edit);
    expect(await memories.removeCoreEntry('agent/a', 'notes', 'mem 1')).toEqual(
      edit,
    );
    expect(
      requests.map(({ url, init }) => [init?.method ?? 'GET', url]),
    ).toEqual([
      ['GET', '/api/agents/agent%2Fa/core-memory'],
      ['POST', '/api/agents/agent%2Fa/core-memory/user'],
      ['PATCH', '/api/agents/agent%2Fa/core-memory/notes/mem%201'],
      ['DELETE', '/api/agents/agent%2Fa/core-memory/notes/mem%201'],
    ]);
    expect(requests[1]?.init?.body).toBe(
      JSON.stringify({ text: 'Short answers' }),
    );
    expect(requests[2]?.init?.body).toBe(
      JSON.stringify({ text: 'Keep it short' }),
    );
    expect(requests[3]?.init?.body).toBeUndefined();
  });

  it('a core memory refusal carries the daemon message', async () => {
    const { memories } = transport(() =>
      Response.json(
        {
          error:
            'About you is full (1,495/1,500 chars). Replace or remove an entry first.',
        },
        { status: 400 },
      ),
    );
    await expect(memories.addCoreEntry('a', 'user', 'x')).rejects.toMatchObject(
      {
        status: 400,
        message: expect.stringContaining('About you is full'),
      },
    );
  });

  it('exportMemories asks for the format and agent, and returns the file as a blob', async () => {
    const { memories, requests } = transport(
      () =>
        new Response('{"version":1}', {
          headers: { 'content-type': 'application/json' },
        }),
    );

    const all = await memories.exportMemories();
    expect(await all.text()).toBe('{"version":1}');
    await memories.exportMemories({ format: 'md', agentId: 'agent/a' });
    await memories.exportMemories({ format: 'json' });
    expect(requests.map(({ url }) => url)).toEqual([
      '/api/memories/export',
      '/api/memories/export?format=md&agentId=agent%2Fa',
      '/api/memories/export?format=json',
    ]);
    expect(requests[0]?.init?.method ?? 'GET').toBe('GET');
  });

  it('update sends a PATCH with only the given fields', async () => {
    const { memories, requests } = transport(() =>
      Response.json({ id: 'm1', content: 'new' }),
    );

    await memories.update('m1', { content: 'new', importance: 0.5 });

    expect(requests[0].url).toBe('/api/memories/m1');
    expect(requests[0].init?.method).toBe('PATCH');
    expect(JSON.parse(String(requests[0].init?.body))).toEqual({
      content: 'new',
      importance: 0.5,
    });
  });

  it('update can clear tags with null', async () => {
    const { memories, requests } = transport(() => Response.json({ id: 'm1' }));

    await memories.update('m1', { tags: null });
    await memories.update('m1', { importance: 1, tags: undefined });

    expect(String(requests[0].init?.body)).toBe('{"tags":null}');
    expect(String(requests[1].init?.body)).toBe('{"importance":1}');
  });

  it('delete encodes the id and returns the counts', async () => {
    const counts = {
      id: 'a/b',
      removedRelationships: 1,
      updatedRelationships: 2,
      updatedFacts: 3,
    };
    const { memories, requests } = transport(() => Response.json(counts));

    expect(await memories.delete('a/b')).toEqual(counts);
    expect(requests[0].url).toBe('/api/memories/a%2Fb');
    expect(requests[0].init?.method).toBe('DELETE');
    expect(requests[0].init?.body).toBeUndefined();
  });

  it('delete can send the exact expected content, including empty text', async () => {
    const { memories, requests } = transport(() => Response.json({ id: 'm1' }));

    await memories.delete('m1', '');
    await memories.delete('m1', ' Reviewed 🫖 ');

    expect(requests.map(({ init }) => JSON.parse(String(init?.body)))).toEqual([
      { expectedContent: '' },
      { expectedContent: ' Reviewed 🫖 ' },
    ]);
  });

  it('facts sends only the options that are set', async () => {
    const { memories, requests } = transport(() =>
      Response.json({ facts: [fact] }),
    );

    expect(await memories.facts()).toEqual([fact]);
    await memories.facts({ agentId: 'a 1', subject: 'Ana', limit: 20 });

    expect(requests[0].url).toBe('/api/memories/facts');
    expect(requests[1].url).toBe(
      '/api/memories/facts?agentId=a+1&subject=Ana&limit=20',
    );
  });

  it('facts includes includeInactive only when true', async () => {
    const { memories, requests } = transport(() =>
      Response.json({ facts: [] }),
    );

    await memories.facts({ includeInactive: false });
    await memories.facts({ includeInactive: true });

    expect(requests[0].url).toBe('/api/memories/facts');
    expect(requests[1].url).toBe('/api/memories/facts?includeInactive=true');
  });

  it('replaceFact patches the value', async () => {
    const replaced = { fact, superseded: { ...fact, status: 'superseded' } };
    const { memories, requests } = transport(() => Response.json(replaced));

    expect(await memories.replaceFact('fact/1', 'coffee')).toEqual(replaced);
    expect(requests[0].url).toBe('/api/memories/facts/fact%2F1');
    expect(requests[0].init?.method).toBe('PATCH');
    expect(JSON.parse(String(requests[0].init?.body))).toEqual({
      value: 'coffee',
    });
  });

  it('deleteFact deletes by id', async () => {
    const { memories, requests } = transport(() =>
      Response.json({ id: 'fact-1' }),
    );

    expect(await memories.deleteFact('fact-1')).toEqual({ id: 'fact-1' });
    expect(requests[0].url).toBe('/api/memories/facts/fact-1');
    expect(requests[0].init?.method).toBe('DELETE');
  });

  it('deleteEntity sends the kind and encodes ids with colons', async () => {
    const result = {
      kind: 'external',
      id: 'ext:42',
      removedRelationships: 0,
      removedFacts: 1,
    };
    const { memories, requests } = transport(() => Response.json(result));

    expect(await memories.deleteEntity('external', 'ext:42')).toEqual(result);
    expect(requests[0].url).toBe(
      '/api/memories/entities/ext%3A42?kind=external',
    );
    expect(requests[0].init?.method).toBe('DELETE');
  });

  it('a daemon refusal reaches the caller as DaemonHttpError', async () => {
    const { memories } = transport(() =>
      Response.json(
        { error: 'Only an active fact with a value can be edited' },
        { status: 409 },
      ),
    );

    const error = await memories.replaceFact('f', 'v').catch((e) => e);

    expect(error).toBeInstanceOf(DaemonHttpError);
    expect(error.status).toBe(409);
    expect(error.message).toBe(
      'Only an active fact with a value can be edited',
    );
  });

  it('the limits match the daemon', () => {
    expect([
      MAX_MEMORY_EDIT_CHARS,
      MAX_MEMORY_TAGS,
      MAX_MEMORY_TAG_CHARS,
      MAX_FACT_VALUE_CHARS,
      MAX_FACTS_SHOWN,
    ]).toEqual([8000, 20, 40, 500, 500]);
  });
});

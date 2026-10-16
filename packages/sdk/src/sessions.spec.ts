import { describe, expect, it } from 'vitest';

import {
  createDaemonClient,
  DaemonHttpError,
  DaemonTooOldError,
  type SessionMessageAttachment,
  type SessionMessageXlsxExtraction,
} from './index.js';

function transport(respond: (url: string) => Response) {
  const requests: { url: string; init?: RequestInit }[] = [];
  const client = createDaemonClient({
    baseUrl: '',
    fetch: async (url, init) => {
      requests.push({ url: String(url), init });
      return respond(String(url));
    },
  });
  return { sessions: client.sessions, requests };
}

const session = {
  id: 'chat:1',
  agentId: 'agent/a',
  roomId: 'chat:1',
  kind: 'chat',
  title: 'Plans',
};

describe('sessions client', () => {
  it('preserves the exported counts-only XLSX extraction in selected message attachments', async () => {
    const extraction: SessionMessageXlsxExtraction = {
      version: 2,
      format: 'xlsx',
      status: 'partial',
      sheetCount: 2,
      observedCellCount: 6,
      includedCellCount: 4,
      hiddenSheetCount: 1,
      omissions: ['stored_cells_only', 'cell_limit', 'cell_text_limit'],
    };
    const attachment: SessionMessageAttachment = {
      type: 'file',
      kind: 'document',
      id: 'att_xlsx',
      name: 'selected.xlsx',
      mime: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
      extraction,
    };
    const { sessions, requests } = transport(() =>
      Response.json({
        messages: [
          {
            id: 'message',
            role: 'user',
            text: '',
            attachments: [attachment],
            metadata: {},
            createdAtMs: 1,
          },
        ],
        nextBefore: null,
      }),
    );

    const page = await sessions.messages('agent/a', 'chat:1');
    const summary = page.messages[0].attachments[0].extraction;
    expect(summary).toEqual(extraction);
    if (summary && 'format' in summary && summary.format === 'xlsx') {
      const xlsx: SessionMessageXlsxExtraction = summary;
      expect(xlsx.includedCellCount).toBe(4);
    } else {
      throw new Error('Expected the XLSX extraction variant');
    }
    expect(requests[0].url).toBe(
      '/api/agents/agent%2Fa/sessions/chat%3A1/messages',
    );
  });

  it('preserves legacy and uploaded message attachments in the same page', async () => {
    const legacy: SessionMessageAttachment = {
      type: 'file',
      name: 'legacy.txt',
    };
    const uploaded: SessionMessageAttachment = {
      type: 'file',
      name: 'selected.docx',
      id: 'att_1',
      mime: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
      size: 120,
      kind: 'document',
      path: 'attachments/selected.docx',
      createdAtMs: 1,
      extraction: {
        version: 1,
        status: 'partial',
        paragraphCount: 2,
        omissions: ['main_body_only', 'context_limit'],
      },
    };
    const pdf: SessionMessageAttachment = {
      type: 'file',
      name: 'selected.pdf',
      id: 'att_pdf',
      mime: 'application/pdf',
      size: 240,
      kind: 'document',
      extraction: {
        version: 2,
        format: 'pdf',
        status: 'partial',
        pageCount: 2,
        textPageCount: 1,
        includedPageCount: 1,
        omissions: ['text_layer_only'],
      },
    };
    const { sessions } = transport(() =>
      Response.json({
        messages: [
          {
            id: 'message',
            role: 'user',
            text: '',
            attachments: [legacy, uploaded, pdf],
            metadata: {},
            createdAtMs: 1,
          },
        ],
        nextBefore: null,
      }),
    );
    const page = await sessions.messages('agent', 'session');
    expect(page.messages[0].attachments).toEqual([legacy, uploaded, pdf]);
    expect(page.messages[0].attachments[0]).not.toHaveProperty('id');
    expect(page.messages[0].attachments[1].extraction).toEqual(
      uploaded.extraction,
    );
    expect(page.messages[0].attachments[2].extraction).toEqual(pdf.extraction);
  });
  it('lists sessions with encoded filters', async () => {
    const page = { sessions: [session], nextCursor: 'next' };
    const { sessions, requests } = transport(() => Response.json(page));

    expect(
      await sessions.list('agent/a', {
        kind: 'chat',
        archived: false,
        q: 'budget plan',
        cursor: 'abc',
        limit: 20,
        includeHelpers: false,
      }),
    ).toEqual(page);
    await sessions.list('agent/a');

    expect(requests.map(({ url }) => url)).toEqual([
      '/api/agents/agent%2Fa/sessions?kind=chat&archived=false&q=budget+plan&cursor=abc&limit=20&includeHelpers=false',
      '/api/agents/agent%2Fa/sessions',
    ]);
  });

  it('reads one session and a message page with an encoded session id', async () => {
    const { sessions, requests } = transport((url) =>
      url.includes('/messages')
        ? Response.json({ messages: [{ id: 'm1' }], nextBefore: 'm1' })
        : Response.json({ session }),
    );

    expect(await sessions.get('agent/a', 'chat:1')).toEqual(session);
    expect(
      await sessions.messages('agent/a', 'chat:1', {
        before: 'm2',
        limit: 10,
        includeHidden: true,
      }),
    ).toEqual({ messages: [{ id: 'm1' }], nextBefore: 'm1' });
    await sessions.messages('agent/a', 'chat:1');

    expect(requests.map(({ url }) => url)).toEqual([
      '/api/agents/agent%2Fa/sessions/chat%3A1',
      '/api/agents/agent%2Fa/sessions/chat%3A1/messages?before=m2&limit=10&includeHidden=true',
      '/api/agents/agent%2Fa/sessions/chat%3A1/messages',
    ]);
  });

  it('a session read keeps its usage totals', async () => {
    const usage = {
      calls: 2,
      promptTokens: 10,
      completionTokens: 5,
      cachedPromptTokens: 0,
      reasoningTokens: 0,
      totalTokens: 15,
      costMicros: 42,
      unpricedCalls: 0,
      subscriptionCalls: 0,
    };
    const { sessions } = transport(() =>
      Response.json({ session: { ...session, usage } }),
    );

    const read = await sessions.get('agent/a', 'chat:1');

    expect(read.usage).toEqual(usage);
  });

  it('creates, updates, and removes sessions', async () => {
    const { sessions, requests } = transport((url) =>
      url.endsWith('chat%3A1') && requests.at(-1)?.init?.method === 'DELETE'
        ? Response.json({ deleted: true })
        : Response.json({ session }),
    );

    expect(await sessions.create('agent/a', { title: 'Trip' })).toEqual(
      session,
    );
    expect(await sessions.create('agent/a')).toEqual(session);
    expect(
      await sessions.update('agent/a', 'chat:1', {
        archived: true,
        lastReadAtMs: 5,
      }),
    ).toEqual(session);
    await expect(sessions.remove('agent/a', 'chat:1')).resolves.toBeUndefined();

    expect(
      requests.map(({ url, init }) => [
        init?.method,
        url,
        init?.body === undefined ? undefined : JSON.parse(String(init.body)),
      ]),
    ).toEqual([
      ['POST', '/api/agents/agent%2Fa/sessions', { title: 'Trip' }],
      ['POST', '/api/agents/agent%2Fa/sessions', {}],
      [
        'PATCH',
        '/api/agents/agent%2Fa/sessions/chat%3A1',
        { archived: true, lastReadAtMs: 5 },
      ],
      ['DELETE', '/api/agents/agent%2Fa/sessions/chat%3A1', undefined],
    ]);
  });

  it('exports Markdown as text and surfaces daemon errors', async () => {
    const { sessions, requests } = transport((url) =>
      url.includes('missing')
        ? Response.json({ error: 'not found' }, { status: 404 })
        : new Response('# Plans\n', {
            headers: { 'content-type': 'text/markdown; charset=utf-8' },
          }),
    );

    expect(await sessions.exportMarkdown('agent/a', 'chat:1')).toBe(
      '# Plans\n',
    );
    expect(requests[0].url).toBe(
      '/api/agents/agent%2Fa/sessions/chat%3A1/export',
    );
    expect(
      (requests[0].init?.headers as Record<string, string>).accept,
    ).toContain('text/markdown');

    const failure = sessions.exportMarkdown('agent/a', 'chat:missing');
    await expect(failure).rejects.toBeInstanceOf(DaemonHttpError);
    await expect(failure).rejects.toMatchObject({
      status: 404,
      message: 'not found',
    });
  });

  // Controller ruling 2 (M2 pre-flight audit): a 404 from the sessions route
  // is ambiguous by itself (an unknown route and a real not-found answer the
  // same JSON 404), so `list` alone probes `GET /api/agents/{agentId}` to
  // tell "daemon too old" apart from "agent not found".
  it('raises a too-old error when sessions 404s but the agent exists', async () => {
    const { sessions, requests } = transport((url) =>
      url === '/api/agents/agent%2Fa/sessions'
        ? Response.json({ error: 'not found' }, { status: 404 })
        : Response.json({ agent: { state: { id: 'agent/a' } } }),
    );

    const failure = sessions.list('agent/a');
    await expect(failure).rejects.toBeInstanceOf(DaemonTooOldError);
    await expect(failure).rejects.toMatchObject({ code: 'daemon_too_old' });
    expect(requests.map(({ url }) => url)).toEqual([
      '/api/agents/agent%2Fa/sessions',
      '/api/agents/agent%2Fa',
    ]);
  });

  it('keeps the original not-found when both the sessions route and the agent 404', async () => {
    const { sessions, requests } = transport(() =>
      Response.json({ error: 'not found' }, { status: 404 }),
    );

    const failure = sessions.list('agent/missing');
    await expect(failure).rejects.toBeInstanceOf(DaemonHttpError);
    await expect(failure).rejects.not.toBeInstanceOf(DaemonTooOldError);
    await expect(failure).rejects.toMatchObject({
      status: 404,
      message: 'not found',
    });
    expect(requests.map(({ url }) => url)).toEqual([
      '/api/agents/agent%2Fmissing/sessions',
      '/api/agents/agent%2Fmissing',
    ]);
  });

  it('compacts a session and returns its record', async () => {
    const { sessions, requests } = transport(() =>
      Response.json({ session }, { status: 202 }),
    );

    expect(await sessions.compact('agent/a', 'chat:1')).toEqual(session);
    expect(requests[0].url).toBe(
      '/api/agents/agent%2Fa/sessions/chat%3A1/compact',
    );
    expect(requests[0].init?.method).toBe('POST');
  });
});

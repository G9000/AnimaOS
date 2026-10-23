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
  it('creates a revision using the full native history and an optional message boundary', async () => {
    const revision = {
      session: { ...session, id: 'branch' },
      retryPrompt: 'Try this',
      attachmentsOmitted: true,
      notice: 'Review before sending.',
    };
    const { sessions, requests } = transport(() => Response.json(revision));
    expect(
      await sessions.revise('agent/a', 'chat:1', {
        mode: 'branch',
        throughMessageId: 'message/2',
      }),
    ).toEqual(revision);
    expect(requests[0].url).toBe(
      '/api/agents/agent%2Fa/sessions/chat%3A1/revise',
    );
    expect(requests[0].init?.method).toBe('POST');
    expect(JSON.parse(String(requests[0].init?.body))).toEqual({
      mode: 'branch',
      throughMessageId: 'message/2',
    });
  });

  it('serializes focused compaction without changing the empty options default', async () => {
    const { sessions, requests } = transport(() => Response.json({ session }));
    await sessions.compact('agent/a', 'chat:1', {
      keepLatestExchanges: 3,
      focus: 'Outstanding work',
    });
    await sessions.compact('agent/a', 'chat:1');
    expect(JSON.parse(String(requests[0].init?.body))).toEqual({
      keepLatestExchanges: 3,
      focus: 'Outstanding work',
    });
    expect(requests[1].init?.body).toBeUndefined();
  });

  it('edits a waiting prompt and distinguishes unchanged order from move to end', async () => {
    const run = { id: 'run/1', input: { text: 'edited' }, queueOrder: 2 };
    const { sessions, requests } = transport(() => Response.json({ run }));
    expect(
      await sessions.updateQueuedRun('agent/a', 'chat:1', 'run/1', {
        text: 'edited',
      }),
    ).toEqual(run);
    await sessions.updateQueuedRun('agent/a', 'chat:1', 'run/1', {
      beforeRunId: null,
    });
    expect(requests[0].url).toBe(
      '/api/agents/agent%2Fa/sessions/chat%3A1/queue/run%2F1',
    );
    expect(requests[0].init?.method).toBe('PATCH');
    expect(JSON.parse(String(requests[0].init?.body))).toEqual({
      text: 'edited',
    });
    expect(JSON.parse(String(requests[1].init?.body))).toEqual({
      beforeRunId: null,
    });
  });
  it('searches individual saved messages with a scoped encoded query and cancellation', async () => {
    const page = {
      results: [
        {
          sessionId: 'chat:1',
          title: 'Plans',
          messageId: 'old-1',
          role: 'user',
          createdAtMs: 1,
          snippet: 'cobalt plan',
        },
        {
          sessionId: 'chat:1',
          title: 'Plans',
          messageId: 'old-2',
          role: 'assistant',
          createdAtMs: 2,
          snippet: 'cobalt answer',
        },
      ],
      hasMore: true,
    };
    const { sessions, requests } = transport(() => Response.json(page));
    const controller = new AbortController();
    expect(
      await sessions.searchMessages('agent/a', {
        q: 'cobalt & plan',
        limit: 20,
        signal: controller.signal,
      }),
    ).toEqual(page);
    expect(requests).toHaveLength(1);
    expect(requests[0].url).toBe(
      '/api/agents/agent%2Fa/message-search?q=cobalt+%26+plan&limit=20',
    );
    expect(requests[0].init?.signal).toBe(controller.signal);
  });

  it('reads an exact historical message window without changing ordinary paging', async () => {
    const page = {
      messages: [],
      nextBefore: 'older',
      anchorMessageId: 'msg/a ?1',
      hasNewer: true,
    };
    const { sessions, requests } = transport(() => Response.json(page));
    expect(
      await sessions.messages('agent/a', 'chat:1', { anchor: 'msg/a ?1' }),
    ).toEqual(page);
    expect(requests[0].url).toBe(
      '/api/agents/agent%2Fa/sessions/chat%3A1/messages?anchor=msg%2Fa+%3F1',
    );
  });

  it('surfaces message search failure without a legacy fallback', async () => {
    const { sessions, requests } = transport(() =>
      Response.json({ error: 'history unavailable' }, { status: 503 }),
    );
    await expect(
      sessions.searchMessages('agent/a', { q: 'cobalt' }),
    ).rejects.toMatchObject({ status: 503, message: 'history unavailable' });
    expect(requests).toHaveLength(1);
  });

  it.each([200, 201])(
    'resolves the main chat through the encoded sibling route (%s)',
    async (status) => {
      const main = { ...session, mainChat: true };
      const { sessions, requests } = transport(() =>
        Response.json({ session: main }, { status }),
      );

      expect(await sessions.main('agent/a ?')).toEqual(main);
      expect(requests).toHaveLength(1);
      expect(requests[0].url).toBe('/api/agents/agent%2Fa%20%3F/main-chat');
      expect(requests[0].init?.method).toBe('POST');
      expect(JSON.parse(String(requests[0].init?.body))).toEqual({});
    },
  );

  it.each([401, 409, 503])(
    'surfaces main-chat errors without creating a session (%s)',
    async (status) => {
      const { sessions, requests } = transport(() =>
        Response.json({ error: 'main chat unavailable' }, { status }),
      );

      await expect(sessions.main('agent/a')).rejects.toMatchObject({
        status,
        message: 'main chat unavailable',
      });
      expect(requests.map(({ url }) => url)).toEqual([
        '/api/agents/agent%2Fa/main-chat',
      ]);
    },
  );

  it('requires a daemon update when main-chat is missing for an existing agent', async () => {
    const { sessions, requests } = transport((url) =>
      url.endsWith('/main-chat')
        ? Response.json({ error: 'not found' }, { status: 404 })
        : Response.json({ agent: { state: { id: 'agent/a' } } }),
    );

    const failure = sessions.main('agent/a');
    await expect(failure).rejects.toBeInstanceOf(DaemonTooOldError);
    await expect(failure).rejects.toMatchObject({ code: 'daemon_too_old' });
    await expect(failure).rejects.toThrow(/main chat.*update the daemon/i);
    expect(requests.map(({ url }) => url)).toEqual([
      '/api/agents/agent%2Fa/main-chat',
      '/api/agents/agent%2Fa',
    ]);
  });

  it('keeps main-chat not-found when the agent is also missing', async () => {
    const { sessions, requests } = transport(() =>
      Response.json({ error: 'agent not found' }, { status: 404 }),
    );

    const failure = sessions.main('missing');
    await expect(failure).rejects.toBeInstanceOf(DaemonHttpError);
    await expect(failure).rejects.not.toBeInstanceOf(DaemonTooOldError);
    expect(requests.map(({ url }) => url)).toEqual([
      '/api/agents/missing/main-chat',
      '/api/agents/missing',
    ]);
  });

  it('m83 preserves the narrow saved source projection and legacy messages without searching on reload', async () => {
    const sources = [
      { title: 'Returned source', url: 'https://example.com/exact?x=1&y=2' },
    ];
    const { sessions, requests } = transport(() =>
      Response.json({
        messages: [
          {
            id: 'tool',
            role: 'tool',
            text: 'canonical native result',
            attachments: [],
            metadata: {
              runId: 'run',
              stepId: 'run:1',
              toolCallId: 'call',
              toolStatus: 'success',
              webSources: sources,
            },
            createdAtMs: 1,
          },
          {
            id: 'legacy',
            role: 'tool',
            text: 'legacy result',
            attachments: [],
            metadata: {},
            createdAtMs: 2,
          },
        ],
        nextBefore: null,
      }),
    );
    const page = await sessions.messages('agent', 'session');
    expect(page.messages[0].metadata.webSources).toEqual(sources);
    expect(page.messages[1].metadata).not.toHaveProperty('webSources');
    expect(requests.map((request) => request.url)).toEqual([
      '/api/agents/agent/sessions/session/messages',
    ]);
  });

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

import { expect, it } from 'vitest';
import { createDaemonClient } from './client.js';

it('requests an exact inspection and binds report access to the current owner chat', async () => {
  const requests: { url: string; init?: RequestInit }[] = [];
  const client = createDaemonClient({ baseUrl: '', fetch: async (url, init) => {
    requests.push({ url: String(url), init });
    return Response.json({ version: 1 });
  }});
  const signal = new AbortController().signal;
  const subject = { kind: 'workspace' as const, file_path: 'reports/a.docx', expected_sha256: 'a'.repeat(64) };
  await client.documentInspections.inspect(subject, 'agent/a', 'chat:1', { signal });
  await client.documentInspections.get('b'.repeat(64), 'agent/a', 'chat:1', { signal });
  expect(requests[0]?.url).toBe('/api/document-inspections?agent_id=agent%2Fa&session_id=chat%3A1');
  expect(requests[0]?.init?.method).toBe('POST');
  expect(JSON.parse(String(requests[0]?.init?.body))).toEqual({ subject });
  expect(requests[0]?.init?.signal).toBe(signal);
  expect(requests[1]?.url).toBe(`/api/document-inspections/${'b'.repeat(64)}?agent_id=agent%2Fa&session_id=chat%3A1`);
});

it('fetches a bounded authenticated PNG and refuses invalid page numbers', async () => {
  const urls: string[] = [];
  const client = createDaemonClient({ baseUrl: '', fetch: async (url) => {
    urls.push(String(url));
    return new Response(new Uint8Array([137, 80, 78, 71]), { headers: { 'content-type': 'image/png', 'content-length': '4' } });
  }});
  await expect(client.documentInspections.page('a'.repeat(64), 0, 'agent', 'chat')).rejects.toThrow('Invalid inspection page');
  const blob = await client.documentInspections.page('a'.repeat(64), 1, 'agent', 'chat');
  expect(blob.size).toBe(4);
  expect(urls).toEqual([`/api/document-inspections/${'a'.repeat(64)}/pages/1?agent_id=agent&session_id=chat`]);
});

it('surfaces the native explanation when inspection is inconclusive', async () => {
  const reason = 'Document inspection engine is unavailable';
  const client = createDaemonClient({ baseUrl: '', fetch: async () =>
    Response.json({ kind: 'inconclusive', reason, error: reason }, { status: 400 }),
  });
  await expect(client.documentInspections.inspect(
    { kind: 'workspace', file_path: 'report.docx', expected_sha256: 'a'.repeat(64) },
    'agent', 'chat',
  )).rejects.toThrow(reason);
});

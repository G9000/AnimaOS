import { test, expect } from '@playwright/test';
import { openConsole } from './fixtures/journeys';
import type { BrowserStreams } from './fixtures/stream';

test('abort and reader cancellation detach browser-local stream ownership', async ({
  page,
}) => {
  const fixture = await openConsole(page);
  const counts = await page.evaluate(async () => {
    const streams = (
      globalThis as unknown as { consoleStreams: BrowserStreams }
    ).consoleStreams;
    const signal = new AbortController();
    const response = await fetch('/api/agents/companion/events', {
      signal: signal.signal,
    });
    const reader = response.body!.getReader();
    await reader.read();
    const opened = streams.count();
    const pending = reader.read().catch((error) => error.name);
    signal.abort();
    const rejection = await pending;
    const aborted = streams.count();
    const another = await fetch('/api/agents/companion/events');
    await another.body!.cancel();
    return { opened, aborted, cancelled: streams.count(), rejection };
  });
  expect(counts).toEqual({
    opened: 2,
    aborted: 1,
    cancelled: 1,
    rejection: 'AbortError',
  });
  fixture.assertClean();
});
test('unknown fixture method and endpoint fail visibly and are recorded', async ({
  page,
}) => {
  const fixture = await openConsole(page);
  const status = await page.evaluate(
    async () => (await fetch('/api/unknown', { method: 'DELETE' })).status,
  );
  expect(status).toBe(501);
  expect(fixture.unexpected).toEqual(['DELETE /api/unknown']);
  expect(() => fixture.assertClean()).toThrow('Unexpected fixture requests');
});

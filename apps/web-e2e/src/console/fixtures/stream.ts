/// <reference lib="dom" />
import type { Page } from '@playwright/test';
import type { AgentEvent } from '@animaos/sdk';
import { AGENT, ORIGIN, snapshot, type ConsoleFixtureState } from './state';
export interface BrowserStreams {
  emit(event: AgentEvent): void;
  disconnect(): void;
  count(): number;
}
/** A flat transport boundary keeps Playwright's Unboxed type from expanding
 *  the recursive JSON criteria reachable through AgentEvent -> Run.outcome. */
interface SerializedAgentEvent {
  seq: number;
  type: AgentEvent['type'];
  serialized: string;
}
export async function installStreams(
  page: Page,
  state: ConsoleFixtureState,
  onConnection: () => void,
  origin = ORIGIN,
) {
  await page.exposeBinding('consoleStreamSnapshot', () => {
    onConnection();
    return snapshot(state);
  });
  await page.addInitScript(
    ({ origin, agentId }) => {
      const originalFetch = globalThis.fetch.bind(globalThis);
      const streams = new Map<
        ReadableStreamDefaultController<Uint8Array>,
        () => void
      >();
      const encode = (event: { seq: number; type: string }) =>
        new TextEncoder().encode(
          `id: ${event.seq}\nevent: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`,
        );
      const fixture = globalThis as unknown as {
        consoleStreamSnapshot(): Promise<{ seq: number; type: string }>;
        consoleStreams: BrowserStreams;
      };
      fixture.consoleStreams = {
        emit(event) {
          for (const stream of streams.keys()) stream.enqueue(encode(event));
        },
        disconnect() {
          for (const [stream, detach] of streams) {
            detach();
            stream.close();
          }
        },
        count() {
          return streams.size;
        },
      };
      globalThis.fetch = async (input, init) => {
        const request = new Request(
          new URL(
            typeof input === 'string'
              ? input
              : input instanceof URL
                ? input.href
                : input.url,
            location.href,
          ),
          init,
        );
        const url = new URL(request.url);
        if (
          url.origin !== origin ||
          url.pathname !== `/api/agents/${agentId}/events` ||
          request.method !== 'GET'
        )
          return originalFetch(input, init);
        const first = await fixture.consoleStreamSnapshot();
        if (request.signal.aborted)
          throw new DOMException('Aborted', 'AbortError');
        let controller: ReadableStreamDefaultController<Uint8Array>;
        const detach = () => {
          streams.delete(controller);
          request.signal.removeEventListener('abort', abort);
        };
        const abort = () => {
          detach();
          controller.error(new DOMException('Aborted', 'AbortError'));
        };
        const body = new ReadableStream<Uint8Array>({
          start(next) {
            controller = next;
            streams.set(next, detach);
            next.enqueue(encode(first));
            request.signal.addEventListener('abort', abort, { once: true });
          },
          cancel() {
            detach();
          },
        });
        return new Response(body, {
          status: 200,
          headers: {
            'content-type': 'text/event-stream',
            'cache-control': 'no-store',
          },
        });
      };
    },
    { origin, agentId: AGENT },
  );
}
export async function emit(page: Page, event: AgentEvent) {
  const input: SerializedAgentEvent = {
    seq: event.seq,
    type: event.type,
    serialized: JSON.stringify(event),
  };
  await page.evaluate<void, SerializedAgentEvent>(
    (input) => {
      const parsed: unknown = JSON.parse(input.serialized);
      if (
        typeof parsed !== 'object' || parsed === null || Array.isArray(parsed) ||
        !('seq' in parsed) || parsed.seq !== input.seq ||
        !('type' in parsed) || parsed.type !== input.type
      ) throw new Error('Browser stream event identity changed during serialization');
      // The helper accepts a full AgentEvent above and serialized that exact
      // value. Only its transport envelope crosses Playwright's mapped type.
      (
        globalThis as unknown as { consoleStreams: BrowserStreams }
      ).consoleStreams.emit(parsed as AgentEvent);
    },
    input,
  );
}
export async function disconnect(page: Page) {
  await page.evaluate(() =>
    (
      globalThis as unknown as { consoleStreams: BrowserStreams }
    ).consoleStreams.disconnect(),
  );
}

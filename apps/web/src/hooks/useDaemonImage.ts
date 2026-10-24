import { useEffect, useMemo, useState } from 'react';
import { ownerApiKeyHeaders } from '../lib/owner-api-key';
import { consoleSessionToken, consoleSessionHeaders } from '../lib/console-session';

const MAX_IMAGE_BYTES = 5 * 1024 * 1024;
const IMAGE_TYPES = new Set(['image/png', 'image/jpeg', 'image/webp']);

async function boundedImage(
  response: Response,
  signal: AbortSignal,
): Promise<Blob> {
  const type =
    response.headers
      .get('content-type')
      ?.split(';', 1)[0]
      .trim()
      .toLowerCase() ?? '';
  const length = Number(response.headers.get('content-length'));
  if (
    !response.ok ||
    !IMAGE_TYPES.has(type) ||
    length > MAX_IMAGE_BYTES ||
    signal.aborted ||
    !response.body
  ) {
    await response.body?.cancel().catch(() => undefined);
    throw new Error('Daemon image is unavailable');
  }
  const reader = response.body.getReader();
  const chunks: ArrayBuffer[] = [];
  let size = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (signal.aborted) throw new Error('Daemon image request ended');
      if (done) break;
      size += value.byteLength;
      if (size > MAX_IMAGE_BYTES)
        throw new Error('Daemon image exceeds its size limit');
      const bytes = new Uint8Array(value.byteLength);
      bytes.set(value);
      chunks.push(bytes.buffer);
    }
    if (size === 0) throw new Error('Daemon image is empty');
    return new Blob(chunks, { type });
  } catch (error) {
    await reader.cancel().catch(() => undefined);
    throw error;
  } finally {
    reader.releaseLock();
  }
}

/** Authenticated images use bounded blobs; the owner key never enters a URL. */
export function useDaemonImage(source: string | null): {
  src: string | null;
  failed: boolean;
} {
  const ownerKey = ownerApiKeyHeaders()['x-api-key'];
  const sessionToken = consoleSessionToken();
  const scope = useMemo(() => ({ source, ownerKey, sessionToken }), [source, ownerKey, sessionToken]);
  const [loaded, setLoaded] = useState<{
    scope: object;
    src: string | null;
    failed: boolean;
  } | null>(null);
  useEffect(() => {
    if (!source || (!ownerKey && !sessionToken)) return;
    let live = true;
    let objectUrl: string | null = null;
    const controller = new AbortController();
    const load = async () => {
      try {
        const url = new URL(source, window.location.href);
        if (
          url.origin !== window.location.origin ||
          !url.pathname.startsWith('/api/') ||
          url.username ||
          url.password
        )
          throw new Error('Daemon image must use this origin');
        const response = await fetch(source, {
          headers: {
            accept: 'image/png,image/jpeg,image/webp',
            ...ownerApiKeyHeaders(),
            ...consoleSessionHeaders(),
          },
          credentials: 'same-origin',
          cache: 'no-store',
          redirect: 'error',
          referrerPolicy: 'origin',
          signal: controller.signal,
        });
        const blob = await boundedImage(response, controller.signal);
        if (!live) return;
        objectUrl = URL.createObjectURL(blob);
        setLoaded({ scope, src: objectUrl, failed: false });
      } catch {
        if (live) setLoaded({ scope, src: null, failed: true });
      }
    };
    void load();
    return () => {
      live = false;
      controller.abort();
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [source, ownerKey, sessionToken, scope]);
  if (!ownerKey && !sessionToken) return { src: null, failed: false };
  return loaded?.scope === scope ? loaded : { src: null, failed: false };
}

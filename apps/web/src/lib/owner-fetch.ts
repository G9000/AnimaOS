import { consoleSessionHeaders } from './console-session';
import { ownerApiKeyHeaders } from './owner-api-key';

/** Owner credentials stay on this console origin and never follow redirects. */
export const ownerFetch: typeof fetch = async (input, init) => {
  const address = typeof input === 'string' || input instanceof URL ? String(input) : input.url;
  const target = new URL(address, window.location.href);
  if (target.origin !== window.location.origin || target.username || target.password)
    throw new Error('Owner requests must stay on the console origin.');
  const headers = new Headers(input instanceof Request ? input.headers : undefined);
  new Headers(init?.headers).forEach((value, name) => headers.set(name, value));
  for (const [name, value] of Object.entries({ ...consoleSessionHeaders(), ...ownerApiKeyHeaders() })) headers.set(name, value);
  return globalThis.fetch(input, { ...init, headers, credentials: 'same-origin', redirect: 'error' });
};

import { claimConsoleSession, consoleSessionHeaders, createDaemonClient } from '@animaos/sdk';

let apiKey: string | undefined;
export function setOwnerApiKey(value: string | undefined) { apiKey = value; }
export function ownerHeaders(): Record<string, string> {
  return { ...consoleSessionHeaders(), ...(apiKey ? { 'x-api-key': apiKey } : {}) };
}
export const ownerFetch: typeof fetch = async (input, init) => {
  const address = typeof input === 'string' || input instanceof URL ? String(input) : input.url;
  const url = new URL(address, window.location.href);
  if (url.origin !== window.location.origin || url.username || url.password) throw new Error('Owner requests must stay on the playground origin.');
  const headers = new Headers(init?.headers);
  for (const [name, value] of Object.entries(ownerHeaders())) headers.set(name, value);
  return fetch(input, { ...init, headers, credentials: 'same-origin', redirect: 'error' });
};
export const ownerClient = createDaemonClient({ baseUrl: '', fetch: ownerFetch });
let claim: ReturnType<typeof claimConsoleSession> | undefined;
export async function verifyOwnerSession(): Promise<number> {
  claim ??= claimConsoleSession();
  const result = await claim;
  if (result.kind === 'refused') throw new Error(result.warning);
  const response = await ownerFetch('/api/capabilities', { cache: 'no-store' });
  await response.body?.cancel().catch(() => undefined);
  return response.status;
}

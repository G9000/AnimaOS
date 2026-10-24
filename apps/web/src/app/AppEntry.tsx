import { useEffect, useSyncExternalStore } from 'react';
import { App } from './app';
import { SharedConversation } from '../components/sessions/SharedConversation';
import { claimConsoleSession } from '../lib/console-session';
import { OwnerApiKeyGate } from '../components/OwnerApiKeyGate';
import { clearOwnerApiKey, ownerApiKeyHeaders } from '../lib/owner-api-key';
import { isViewerHash } from '../lib/hash-route';

let ownerClaim: ReturnType<typeof claimConsoleSession> | undefined;
const subscribe = (notify: () => void) => {
  window.addEventListener('hashchange', notify);
  return () => window.removeEventListener('hashchange', notify);
};
const currentHash = () => window.location.hash;

/** A viewer route mounts no owner bootstrap, transport, tool UI or console claim. */
export function AppEntry() {
  const hash = useSyncExternalStore(subscribe, currentHash, () => '');
  useEffect(() => { if (isViewerHash(hash)) clearOwnerApiKey(); }, [hash]);
  if (isViewerHash(hash)) {
    const id = /^#\/shared\/(share-[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12})$/.exec(hash)?.[1];
    return id ? <SharedConversation key={id} shareId={id} /> : <main><p>This shared conversation link is invalid.</p></main>;
  }
  ownerClaim ??= claimConsoleSession(window, (url, init) => globalThis.fetch(url, {
    ...init, headers: { ...ownerApiKeyHeaders(), ...Object.fromEntries(new Headers(init?.headers).entries()) },
  }));
  return <OwnerApiKeyGate sessionClaim={ownerClaim}><App consoleClaim={ownerClaim} /></OwnerApiKeyGate>;
}

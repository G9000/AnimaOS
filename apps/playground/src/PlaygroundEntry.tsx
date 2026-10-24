import { useEffect, useState } from 'react';
import { Playground } from './playground';
import { setOwnerApiKey, verifyOwnerSession } from './lib/owner-session';

export function PlaygroundEntry() {
  const [status, setStatus] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [key, setKey] = useState('');
  const [busy, setBusy] = useState(true);
  useEffect(() => {
    let live = true;
    void verifyOwnerSession().then((status) => { if (live) setStatus(status); }, (error: unknown) => {
      if (live) setError(error instanceof Error ? error.message : 'Owner session could not be opened.');
    }).finally(() => { if (live) setBusy(false); });
    return () => { live = false; };
  }, []);
  if (status === 200) return <Playground />;
  return <main className="mx-auto max-w-md p-8">
    <h1>Open your Anima playground</h1>
    {busy ? <p role="status">Checking owner access…</p> : status === 401 ?
      <form onSubmit={(event) => {
        event.preventDefault(); setBusy(true); setError(null); setOwnerApiKey(key.trim());
        void verifyOwnerSession().then((status) => {
          setStatus(status); if (status !== 200) { setOwnerApiKey(undefined); setError('The owner API key was not accepted.'); }
        }, () => { setOwnerApiKey(undefined); setError('Owner access could not be verified.'); }).finally(() => setBusy(false));
      }}><label>Owner API key<input type="password" autoComplete="off" maxLength={4096} value={key} onChange={(event) => setKey(event.target.value)} /></label><button disabled={busy || !key.trim()}>Unlock playground</button></form>
      : <p>Open a fresh playground link from your local Anima launcher.</p>}
    {error && <p role="alert">{error}</p>}
  </main>;
}

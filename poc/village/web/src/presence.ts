export interface ResidentActivity {
  interactionId: string;
  sourceEventId: string;
  complete: boolean;
  status?: 'pending' | 'completed' | 'budget_exhausted' | 'recovery_required';
  terminalReason?: string | null;
  replies: { eventId: string; npcId: 'tavi' | 'mira'; text: string; translation: string; reading: string }[];
}

/** A server-issued presence ends on page closure; heartbeat loss expires server-side. */
export function connectVillagePresence(
  saveId: string,
  onActivity: (activity: ResidentActivity | null) => void,
  fetcher: typeof fetch = fetch,
): () => void {
  const base = `/api/village/saves/${encodeURIComponent(saveId)}`;
  let session: string | null = null;
  let disposed = false;
  let disabled = false;
  let inFlight = false;
  let timer: ReturnType<typeof setInterval> | undefined;
  const send = (path: string, body: unknown, keepalive = false) =>
    fetcher(`${base}${path}`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body), keepalive,
    });
  const close = () => {
    if (session) {
      void send('/presence/close', { sessionId: session }, true).catch(() => {});
      session = null;
    }
  };
  const open = async () => {
    const response = await send('/presence', {});
    if (!response.ok) {
      if (response.status === 503) {
        const failure = await response.json().catch(() => null) as {error?: unknown} | null;
        // The host explicitly identifies a legacy profile; transient 5xx errors
        // still retry once per interval without opening parallel sessions.
        disabled = failure?.error === 'Resident activity is not enabled for this profile';
      } else if (response.status === 409 || response.status === 404) {
        disabled = true;
      }
      return;
    }
    const result = (await response.json()) as { sessionId?: unknown };
    if (typeof result.sessionId !== 'string' || !result.sessionId) return;
    session = result.sessionId;
    // A late response must not leave a resident active after the page closes.
    if (disposed) close();
  };
  const poll = async () => {
    if (disposed || disabled || inFlight) return;
    inFlight = true;
    try {
      if (!session) {
        await open();
        return;
      }
      const heartbeat = await send('/presence/heartbeat', { sessionId: session });
      if (disposed) return;
      if (heartbeat.status === 410) {
        session = null;
        // Renew an expired session once; failed opens retry on the next tick.
        await open();
        return;
      }
      if (!heartbeat.ok) return;
      const response = await fetcher(`${base}/activity`);
      if (response.ok) {
        const activity = (await response.json()) as ResidentActivity | null;
        if (!disposed) onActivity(activity);
      }
    } catch { /* The server expiry stops new work when heartbeats stop arriving. */ }
    finally { inFlight = false; }
  };
  // Start the bounded retry cadence even when the first request fails. The
  // initial open shares the same single-inflight gate as every later tick.
  timer = setInterval(() => { void poll(); }, 10_000);
  void poll();
  const stop = () => {
    disposed = true;
    if (timer !== undefined) clearInterval(timer);
    close();
    window.removeEventListener('pagehide', stop);
  };
  window.addEventListener('pagehide', stop);
  return stop;
}

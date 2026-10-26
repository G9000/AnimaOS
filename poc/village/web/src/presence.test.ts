import { afterEach, expect, it, vi } from 'vitest';
import { connectVillagePresence } from './presence';
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });
it('heartbeats only an open game session and closes it on cleanup', async () => {
  vi.useFakeTimers();
  const fetch = vi.fn().mockResolvedValue({ ok: true, status: 200, json: async () => ({ sessionId: 'server-session' }) });
  const stop = connectVillagePresence('one', () => {}, fetch);
  await vi.advanceTimersByTimeAsync(0);
  expect(fetch.mock.calls[0]![0]).toBe('/api/village/saves/one/presence');
  await vi.advanceTimersByTimeAsync(10_000);
  expect(fetch.mock.calls.some(([url]) => String(url).endsWith('/heartbeat'))).toBe(true);
  stop();
  expect(fetch.mock.calls.at(-1)![0]).toBe('/api/village/saves/one/presence/close');
  const count=fetch.mock.calls.length;
  await vi.advanceTimersByTimeAsync(60_000);
  expect(fetch.mock.calls).toHaveLength(count);
});
it('does not activate activity on a legacy profile', async () => {
  vi.useFakeTimers();
  const fetch=vi.fn().mockResolvedValue({ok:false,status:503,json:async()=>({error:'Resident activity is not enabled for this profile'})});
  const stop=connectVillagePresence('legacy',()=>{},fetch);
  await vi.advanceTimersByTimeAsync(60_000);
  expect(fetch.mock.calls).toHaveLength(1);
  stop();
});

it('reopens expired presence and resumes polling with the replacement session', async () => {
  vi.useFakeTimers();
  const activity = { interactionId: 'supper', sourceEventId: 'delivery', complete: false, replies: [] };
  const fetch = vi.fn()
    .mockResolvedValueOnce({ ok: true, json: async () => ({ sessionId: 'expired' }) })
    .mockResolvedValueOnce({ ok: false, status: 410 })
    .mockResolvedValueOnce({ ok: true, json: async () => ({ sessionId: 'replacement' }) })
    .mockResolvedValueOnce({ ok: true })
    .mockResolvedValueOnce({ ok: true, json: async () => activity })
    .mockResolvedValue({ ok: true });
  const onActivity = vi.fn();
  const stop = connectVillagePresence('one', onActivity, fetch);
  try {
    await vi.advanceTimersByTimeAsync(20_000);
    expect(fetch.mock.calls.map(([url]) => url)).toEqual([
      '/api/village/saves/one/presence',
      '/api/village/saves/one/presence/heartbeat',
      '/api/village/saves/one/presence',
      '/api/village/saves/one/presence/heartbeat',
      '/api/village/saves/one/activity',
    ]);
    expect(JSON.parse(fetch.mock.calls[3]![1].body)).toEqual({ sessionId: 'replacement' });
    expect(onActivity).toHaveBeenCalledWith(activity);
  } finally { stop(); }
});

it('bounds failed reopen attempts to one per heartbeat interval', async () => {
  vi.useFakeTimers();
  const fetch = vi.fn()
    .mockResolvedValueOnce({ ok: true, json: async () => ({ sessionId: 'expired' }) })
    .mockResolvedValueOnce({ ok: false, status: 410 })
    .mockRejectedValue(new Error('offline'));
  const stop = connectVillagePresence('one', () => {}, fetch);
  try {
    await vi.advanceTimersByTimeAsync(10_000);
    expect(fetch).toHaveBeenCalledTimes(3);
    await vi.advanceTimersByTimeAsync(9_999);
    expect(fetch).toHaveBeenCalledTimes(3);
    await vi.advanceTimersByTimeAsync(1);
    expect(fetch).toHaveBeenCalledTimes(4);
    expect(fetch.mock.calls[3]![0]).toBe('/api/village/saves/one/presence');
  } finally { stop(); }
});

it('closes a replacement returned after disposal without restarting activity', async () => {
  vi.useFakeTimers();
  let resolveOpen!: (value: unknown) => void;
  const pendingOpen = new Promise(resolve => { resolveOpen = resolve; });
  const fetch = vi.fn()
    .mockResolvedValueOnce({ ok: true, json: async () => ({ sessionId: 'expired' }) })
    .mockResolvedValueOnce({ ok: false, status: 410 })
    .mockReturnValueOnce(pendingOpen)
    .mockResolvedValue({ ok: true });
  const onActivity = vi.fn();
  const stop = connectVillagePresence('one', onActivity, fetch);
  await vi.advanceTimersByTimeAsync(10_000);
  await vi.advanceTimersByTimeAsync(30_000);
  expect(fetch).toHaveBeenCalledTimes(3);
  stop();
  resolveOpen({ ok: true, json: async () => ({ sessionId: 'late-replacement' }) });
  await vi.advanceTimersByTimeAsync(0);
  expect(fetch.mock.calls.at(-1)![0]).toBe('/api/village/saves/one/presence/close');
  expect(JSON.parse(fetch.mock.calls.at(-1)![1].body)).toEqual({ sessionId: 'late-replacement' });
  const count = fetch.mock.calls.length;
  await vi.advanceTimersByTimeAsync(60_000);
  expect(fetch).toHaveBeenCalledTimes(count);
  expect(onActivity).not.toHaveBeenCalled();
});

it('does not reopen when an expired heartbeat returns after page closure', async () => {
  vi.useFakeTimers();
  let resolveHeartbeat!: (value: unknown) => void;
  const pendingHeartbeat = new Promise(resolve => { resolveHeartbeat = resolve; });
  const fetch = vi.fn()
    .mockResolvedValueOnce({ ok: true, json: async () => ({ sessionId: 'session' }) })
    .mockReturnValueOnce(pendingHeartbeat)
    .mockResolvedValue({ ok: true });
  const stop = connectVillagePresence('one', () => {}, fetch);
  await vi.advanceTimersByTimeAsync(10_000);
  window.dispatchEvent(new Event('pagehide'));
  resolveHeartbeat({ ok: false, status: 410 });
  await vi.advanceTimersByTimeAsync(60_000);
  expect(fetch.mock.calls.filter(([url]) => String(url).endsWith('/presence'))).toHaveLength(1);
  expect(fetch.mock.calls.some(([url]) => String(url).endsWith('/activity'))).toBe(false);
  stop();
});

it('keeps its current session on a temporary server error', async () => {
  vi.useFakeTimers();
  const fetch = vi.fn()
    .mockResolvedValueOnce({ ok: true, json: async () => ({ sessionId: 'session' }) })
    .mockResolvedValueOnce({ ok: false, status: 503 })
    .mockResolvedValueOnce({ ok: true })
    .mockResolvedValueOnce({ ok: true, json: async () => null })
    .mockResolvedValue({ ok: true });
  const stop = connectVillagePresence('one', () => {}, fetch);
  try {
    await vi.advanceTimersByTimeAsync(20_000);
    expect(fetch.mock.calls.filter(([url]) => String(url).endsWith('/presence'))).toHaveLength(1);
    expect(JSON.parse(fetch.mock.calls[2]![1].body)).toEqual({ sessionId: 'session' });
  } finally { stop(); }
});

it('does not reopen a session replaced by another tab or explicitly closed', async () => {
  vi.useFakeTimers();
  const fetch = vi.fn()
    .mockResolvedValueOnce({ ok: true, json: async () => ({ sessionId: 'replaced' }) })
    .mockResolvedValue({ ok: false, status: 409 });
  const onActivity = vi.fn();
  const stop = connectVillagePresence('one', onActivity, fetch);
  try {
    await vi.advanceTimersByTimeAsync(30_000);
    expect(fetch.mock.calls.filter(([url]) => String(url).endsWith('/presence'))).toHaveLength(1);
    expect(fetch.mock.calls.filter(([url]) => String(url).endsWith('/heartbeat'))).toHaveLength(3);
    expect(fetch.mock.calls.some(([url]) => String(url).endsWith('/activity'))).toBe(false);
    expect(onActivity).not.toHaveBeenCalled();
  } finally { stop(); }
});


it.each(['network', 'server', 'malformed'] as const)('retries an initial %s presence failure on the bounded interval', async failure => {
  vi.useFakeTimers();
  const fetch=vi.fn();
  if (failure==='network') fetch.mockRejectedValueOnce(new Error('offline'));
  else if (failure==='server') fetch.mockResolvedValueOnce({ok:false,status:503,json:async()=>({error:'temporarily unavailable'})});
  else fetch.mockResolvedValueOnce({ok:true,status:200,json:async()=>({sessionId:null})});
  const activity={interactionId:'supper',sourceEventId:'delivery',complete:false,replies:[]};
  fetch.mockResolvedValueOnce({ok:true,status:200,json:async()=>({sessionId:'recovered'})})
    .mockResolvedValueOnce({ok:true,status:200})
    .mockResolvedValueOnce({ok:true,status:200,json:async()=>activity})
    .mockResolvedValue({ok:true,status:200});
  const onActivity=vi.fn();
  const stop=connectVillagePresence('one',onActivity,fetch);
  try {
    await vi.advanceTimersByTimeAsync(9_999);
    expect(fetch).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(fetch.mock.calls[1]![0]).toBe('/api/village/saves/one/presence');
    await vi.advanceTimersByTimeAsync(10_000);
    expect(onActivity).toHaveBeenCalledWith(activity);
    expect(JSON.parse(fetch.mock.calls[2]![1].body)).toEqual({sessionId:'recovered'});
  } finally {stop();}
});

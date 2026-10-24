import type { GameView, Language, NpcId, TurnInput } from './types';

export class VillageApiError extends Error {
  constructor(
    public readonly status: number,
    message: string,
  ) {
    super(message);
    this.name = 'VillageApiError';
  }
}

async function request(path: string, body?: unknown): Promise<GameView> {
  const response = await fetch(`/api/village${path}`, {
    method: body === undefined ? 'GET' : 'POST',
    headers:
      body === undefined ? undefined : { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const payload: unknown = await response.json().catch(() => null);
  if (!response.ok) {
    const detail =
      payload && typeof payload === 'object' && 'error' in payload
        ? String(payload.error)
        : `Request failed (${response.status})`;
    throw new VillageApiError(response.status, detail);
  }
  return payload as GameView;
}

export const villageApi = {
  create: (language: Language) => request('/saves', { language }),
  restore: (id: string) => request(`/saves/${encodeURIComponent(id)}`),
  visit: (id: string, requestId: string, npcId?: NpcId) =>
    request(`/saves/${encodeURIComponent(id)}/visit`, {
      requestId,
      ...(npcId ? { npcId } : {}),
    }),
  turn: (id: string, input: TurnInput) =>
    request(`/saves/${encodeURIComponent(id)}/turns`, input),
};

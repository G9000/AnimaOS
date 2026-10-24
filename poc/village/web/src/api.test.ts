import { afterEach, expect, it, vi } from 'vitest';
import { villageApi } from './api';

afterEach(() => vi.unstubAllGlobals());

it('preserves omitted speaker identity for legacy retries and targets Mira explicitly', async () => {
  const fetch = vi.fn().mockResolvedValue({ ok: true, json: async () => ({}) });
  vi.stubGlobal('fetch', fetch);
  await villageApi.visit('one', 'old-request');
  expect(JSON.parse(fetch.mock.calls[0]![1].body)).toEqual({
    requestId: 'old-request',
  });
  await villageApi.visit('one', 'mira-visit', 'mira');
  expect(JSON.parse(fetch.mock.calls[1]![1].body)).toEqual({
    requestId: 'mira-visit',
    npcId: 'mira',
  });
});

import { describe, expect, it, vi } from 'vitest';
import { parseSignup, saveToAirtable } from './waitlist';

describe('parseSignup', () => {
  it('normalizes the email and keeps the source', () => {
    expect(parseSignup({ email: '  Me@Example.COM ', source: 'hero' })).toEqual({
      ok: true,
      signup: { email: 'me@example.com', source: 'hero' },
    });
  });

  it('rejects a missing or malformed email', () => {
    expect(parseSignup({})).toMatchObject({ ok: false, status: 400 });
    expect(parseSignup({ email: 'not-an-email' })).toMatchObject({ ok: false, status: 400 });
    expect(parseSignup(null)).toMatchObject({ ok: false, status: 400 });
  });

  it('quietly drops sign-ups that fill the honeypot', () => {
    expect(parseSignup({ email: 'bot@example.com', website: 'http://spam' })).toMatchObject({ ok: false, status: 200 });
  });
});

describe('saveToAirtable', () => {
  const config = { token: 'pat_test', baseId: 'appTest', table: 'Waitlist' };

  it('upserts one row keyed on Email', async () => {
    const fetchImpl = vi.fn(async () => new Response('{}', { status: 200 }));
    await saveToAirtable(config, { email: 'me@example.com', source: 'hero' }, fetchImpl as unknown as typeof fetch);

    const [url, init] = fetchImpl.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe('https://api.airtable.com/v0/appTest/Waitlist');
    expect(init.method).toBe('PATCH');
    expect((init.headers as Record<string, string>).Authorization).toBe('Bearer pat_test');
    expect(JSON.parse(init.body as string)).toEqual({
      performUpsert: { fieldsToMergeOn: ['Email'] },
      records: [{ fields: { Email: 'me@example.com', Source: 'hero' } }],
      typecast: true,
    });
  });

  it('throws when Airtable refuses', async () => {
    const fetchImpl = vi.fn(async () => new Response('{"error":"INVALID_PERMISSIONS"}', { status: 403 }));
    await expect(saveToAirtable(config, { email: 'me@example.com', source: '' }, fetchImpl as unknown as typeof fetch)).rejects.toThrow('403');
  });
});

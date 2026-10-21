/* Waitlist sign-ups: validate what the form sends, then upsert one Airtable row per email. */

export type AirtableConfig = {
  token: string;
  baseId: string;
  table: string;
};

export type Signup = {
  email: string;
  source: string;
};

export type ParseResult = { ok: true; signup: Signup } | { ok: false; status: number; error: string };

const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
const MAX_EMAIL = 254;
const MAX_SOURCE = 200;

export function parseSignup(body: unknown): ParseResult {
  if (!body || typeof body !== 'object') return { ok: false, status: 400, error: 'Send a JSON body with an email.' };
  const { email, source, website } = body as Record<string, unknown>;
  // honeypot: people never see the website field, bots fill it in. Pretend it worked.
  if (typeof website === 'string' && website.trim() !== '') return { ok: false, status: 200, error: 'ignored' };
  if (typeof email !== 'string') return { ok: false, status: 400, error: 'Enter an email address like you@domain.com.' };
  const clean = email.trim().toLowerCase();
  if (clean.length > MAX_EMAIL || !EMAIL.test(clean)) return { ok: false, status: 400, error: 'Enter an email address like you@domain.com.' };
  const src = typeof source === 'string' ? source.trim().slice(0, MAX_SOURCE) : '';
  return { ok: true, signup: { email: clean, source: src } };
}

// Upsert on Email so joining twice keeps one row.
export async function saveToAirtable(config: AirtableConfig, signup: Signup, fetchImpl: typeof fetch = fetch): Promise<void> {
  const url = `https://api.airtable.com/v0/${encodeURIComponent(config.baseId)}/${encodeURIComponent(config.table)}`;
  const res = await fetchImpl(url, {
    method: 'PATCH',
    headers: { Authorization: `Bearer ${config.token}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({
      performUpsert: { fieldsToMergeOn: ['Email'] },
      records: [{ fields: { Email: signup.email, Source: signup.source } }],
      typecast: true,
    }),
  });
  if (!res.ok) {
    const detail = await res.text().catch(() => '');
    throw new Error(`Airtable responded ${res.status}: ${detail.slice(0, 300)}`);
  }
}

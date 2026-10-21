import type { APIRoute } from 'astro';
import { AIRTABLE_BASE_ID, AIRTABLE_TABLE, AIRTABLE_TOKEN } from 'astro:env/server';
import { parseSignup, saveToAirtable } from '../../lib/waitlist';

export const prerender = false;

const json = (status: number, body: Record<string, unknown>) =>
  new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });

export const POST: APIRoute = async ({ request }) => {
  if (!AIRTABLE_TOKEN || !AIRTABLE_BASE_ID) return json(503, { error: 'The waitlist isn’t open yet. Star the repo and check back soon.' });

  let body: unknown;
  try {
    body = await request.json();
  } catch {
    return json(400, { error: 'Send a JSON body with an email.' });
  }

  const parsed = parseSignup(body);
  if (!parsed.ok) return parsed.status === 200 ? json(200, { ok: true }) : json(parsed.status, { error: parsed.error });

  try {
    await saveToAirtable({ token: AIRTABLE_TOKEN, baseId: AIRTABLE_BASE_ID, table: AIRTABLE_TABLE }, parsed.signup);
  } catch (err) {
    console.error('waitlist: save failed', err);
    return json(502, { error: 'Couldn’t save that just now. Try again in a moment.' });
  }
  return json(200, { ok: true });
};

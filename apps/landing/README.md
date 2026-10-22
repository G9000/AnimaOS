# anima landing

The marketing page and waitlist. Astro, prerendered, with one Vercel function at `/api/waitlist` that writes sign-ups to Airtable so the Airtable key never reaches the browser.

```bash
bun x nx run @animaos/landing:dev
bun x nx run @animaos/landing:test
bun x nx run @animaos/landing:build
```

Without Airtable settings the page works and the form answers "the waitlist isn't open yet" (503).

## Airtable

1. Make a base with a table named `Waitlist` (or set `AIRTABLE_TABLE`) and two fields:
   - `Email`: Email field. Sign-ups upsert on it, so joining twice keeps one row.
   - `Source`: single line text. The `?ref=` query value, or the referrer.
   - Optional: a `Created time` field to see when each person joined.
2. Create a personal access token with the `data.records:write` scope, limited to that base.
3. Copy `.env.example` to `.env` for local dev, or set the same variables in Vercel.

## Vercel

- Import the repo and set **Root Directory** to `apps/landing`. Vercel detects Astro and installs from the workspace root.
- Add `AIRTABLE_TOKEN`, `AIRTABLE_BASE_ID` and, if needed, `AIRTABLE_TABLE` under Environment Variables.
- Set `ANIMAOS_LANDING_SITE` to the public URL once there is a domain (used for canonical links).

## Analytics

Production builds reuse the old marketing site's Umami Cloud website (`92475600-9412-4389-b186-9bd33160041d`). The shared layout loads its pageview tracker; local development does not load it. Accepted waitlist submissions send the `waitlist-signup` event without the email address. Missing or blocked analytics never prevents signup.

## Layout

- `src/pages/index.astro`: the page. The seven demo screens are raw HTML exported from `design/design.pen`, in `src/slides/`.
- `src/scripts/landing.js`: the WebGL hero field, draggable windows, typing preview and the carousel.
- `src/scripts/waitlist.ts`: the form. `src/lib/waitlist.ts` is the validation and Airtable call the endpoint uses.

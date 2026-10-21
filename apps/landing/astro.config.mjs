import { defineConfig, envField } from 'astro/config';
import vercel from '@astrojs/vercel';

// The page is prerendered; only /api/waitlist runs as a Vercel function.
export default defineConfig({
  site: process.env.ANIMAOS_LANDING_SITE,
  output: 'static',
  adapter: vercel(),
  devToolbar: { enabled: false },
  env: {
    schema: {
      AIRTABLE_TOKEN: envField.string({ context: 'server', access: 'secret', optional: true }),
      AIRTABLE_BASE_ID: envField.string({ context: 'server', access: 'secret', optional: true }),
      AIRTABLE_TABLE: envField.string({ context: 'server', access: 'secret', default: 'Waitlist' }),
    },
  },
});

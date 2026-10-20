import { appendFileSync } from 'node:fs';
import { validateReleasePlan } from './package.mjs';

try {
  const tag = validateReleasePlan(JSON.parse(process.env.DIST_PLAN ?? ''));
  if (!process.env.GITHUB_OUTPUT) throw new Error('GITHUB_OUTPUT is required');
  appendFileSync(process.env.GITHUB_OUTPUT, `release_tag=${tag}\n`);
  console.log(`Native assets match release ${tag}`);
} catch (error) {
  console.error(`release-plan: ${error.message}`);
  process.exitCode = 1;
}

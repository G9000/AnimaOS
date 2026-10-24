import { fileURLToPath } from 'node:url';

import { rustDaemonTestEnvironment } from '../src/rust-daemon-tests.js';
import { runTask } from '../src/task.js';

// This wrapper owns its process environment before spawning any test threads.
// The test binaries and the production app builder receive the same fixture key.
const isolated = rustDaemonTestEnvironment(process.env);
for (const key of Object.keys(process.env)) {
  if (!Object.hasOwn(isolated, key)) delete process.env[key];
}
Object.assign(process.env, isolated);

const args = process.argv.slice(2);
if (args[0] === '--') args.shift();

try {
  process.exitCode = await runTask('cargo', ['test', '-p', 'anima-daemon', ...args], {
    cwd: fileURLToPath(new URL('../../..', import.meta.url)),
  });
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
}

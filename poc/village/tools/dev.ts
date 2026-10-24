import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';
import { runManagedProcesses } from '../../../tools/workspace-dev/src/process.js';
import { villageProcessDefinitions } from './config.js';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
try {
  const definitions = villageProcessDefinitions(root, process.env);
  console.log(
    `Lantern Village: http://127.0.0.1:${definitions[1]?.env?.VILLAGE_WEB_PORT}`,
  );
  console.log(
    'Building the Rust game host; the browser connects when it is ready.',
  );
  await runManagedProcesses(definitions);
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
}

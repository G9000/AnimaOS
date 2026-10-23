import { normalizeVitestArgs } from '../src/vitest-args.js';
import { runTask } from '../src/task.js';

try {
  process.exitCode = await runTask(process.execPath, [
    'x',
    'vitest',
    'run',
    ...normalizeVitestArgs(process.argv.slice(2)),
  ]);
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
}

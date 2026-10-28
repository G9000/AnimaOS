import { resolve } from 'node:path';

import { runTask, type TaskOptions } from '../src/task.js';

const args = process.argv.slice(2);
let cwd = process.cwd();
let rustCache: TaskOptions['rustCache'];
let rustCacheMonitor: boolean | undefined;
while (args[0] === '--cwd' || args[0] === '--rust-cache' || args[0] === '--rust-cache-runtime') {
  const flag = args[0];
  const value = args[1];
  if (!value) throw new Error(`${flag} requires a value`);
  if (flag === '--cwd') cwd = resolve(value);
  else if (value === 'dev' || value === 'validation' || value === 'release') {
    rustCache = value;
    rustCacheMonitor = flag !== '--rust-cache-runtime';
  }
  else throw new Error('--rust-cache must be dev, validation, or release');
  args.splice(0, 2);
}
if (args[0] === '--') args.shift();
const command = args.shift();
if (!command)
  throw new Error('Usage: run-task.ts [--cwd directory] [--rust-cache dev|validation|release] -- command [args]');

try {
  process.exitCode = await runTask(command, args, { cwd, rustCache, rustCacheMonitor });
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
}

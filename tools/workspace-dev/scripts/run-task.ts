import { resolve } from 'node:path';

import { runTask } from '../src/task.js';

const args = process.argv.slice(2);
let cwd = process.cwd();
if (args[0] === '--cwd') {
  const directory = args[1];
  if (!directory) throw new Error('--cwd requires a directory');
  cwd = resolve(directory);
  args.splice(0, 2);
}
if (args[0] === '--') args.shift();
const command = args.shift();
if (!command)
  throw new Error('Usage: run-task.ts [--cwd directory] -- command [args]');

try {
  process.exitCode = await runTask(command, args, { cwd });
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
}

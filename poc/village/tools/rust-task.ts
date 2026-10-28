import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { runTask } from '../../../tools/workspace-dev/src/task.js';
import { villageCargoTarget, villagePorts } from './config.js';

const root = fileURLToPath(new URL('..', import.meta.url));
const manifest = resolve(root, 'Cargo.toml');
const args = process.argv.slice(2);
const task = args.shift();
if (args[0] === '--') args.shift();
process.env.CARGO_TARGET_DIR = villageCargoTarget(root, process.env, task === 'test' ? 'validation' : 'dev');
if (task === 'run') {
  const { apiPort } = villagePorts(process.env);
  process.env.VILLAGE_BIND ??= `127.0.0.1:${apiPort}`;
  process.env.VILLAGE_DB_PATH ??= resolve(
    root,
    '.anima-village/village.sqlite3',
  );
}

const commands: Record<string, string[][]> = {
  run: [['run', '--manifest-path', manifest, '-p', 'anima-village-game']],
  build: [['build', '--manifest-path', manifest, '--workspace']],
  test: [['test', '--manifest-path', manifest, '--workspace']],
  fmt: [['fmt', '--manifest-path', manifest, '--all', '--check']],
  lint: [
    ['fmt', '--manifest-path', manifest, '--all', '--check'],
    [
      'clippy',
      '--manifest-path',
      manifest,
      '--workspace',
      '--all-targets',
      '--',
      '-D',
      'clippy::correctness',
    ],
  ],
};

try {
  const commandsForTask = task ? commands[task] : undefined;
  if (!commandsForTask)
    throw new Error(
      'Usage: rust-task.ts run|build|test|fmt|lint [-- extra cargo arguments]',
    );
  if (task === 'run') {
    const code = await runTask('cargo', ['build', '--manifest-path', manifest, '-p', 'anima-village-game'], { cwd: root });
    if (code !== 0) process.exitCode = code;
    else {
      const binary = resolve(process.env.CARGO_TARGET_DIR!, 'debug', process.platform === 'win32' ? 'anima-village-game.exe' : 'anima-village-game');
      process.exitCode = await runTask(binary, args, { cwd: root, rustCache: 'dev', rustCacheMonitor: false });
    }
  } else {
    for (const command of commandsForTask) {
      const code = await runTask('cargo', [...command, ...args], { cwd: root });
      if (code !== 0) {
        process.exitCode = code;
        break;
      }
    }
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
}

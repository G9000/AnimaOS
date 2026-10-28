import { randomUUID } from 'node:crypto';
import { createRequire } from 'node:module';
import { basename, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { spawnManagedProcess, stopManagedProcess } from './process.js';
import { withRustCacheLease } from './rust-cache-maintenance.js';

interface TaskContext {
  workspaceRoot: string;
  id: string;
}

const require = createRequire(import.meta.url);
const { prepareTask, cleanupTask } = require('../task-temp.cjs') as {
  prepareTask(context: TaskContext): Record<string, string>;
  cleanupTask(context: TaskContext): void;
};

export interface TaskOptions {
  cwd?: string;
  rustCache?: 'dev' | 'validation' | 'release';
  rustCacheMonitor?: boolean;
}

/** Run one command with owned scratch, preserving its exit status. */
export async function runTask(
  command: string,
  args: string[],
  options: TaskOptions = {},
): Promise<number> {
  const workspaceRoot = fileURLToPath(new URL('../../..', import.meta.url));
  const cargoCommand = basename(command).replace(/\.exe$/i, '').toLowerCase() === 'cargo';
  const separator = args.indexOf('--');
  const cargoArgs = cargoCommand ? (separator < 0 ? args : args.slice(0, separator)) : [];
  const builds = new Set(['build', 'check', 'test', 'clippy', 'run', 'bench', 'rustc', 'rustdoc', 'doc']);
  const rustCache = options.rustCache ?? (cargoCommand && builds.has(cargoArgs[0])
    ? cargoArgs[0] === 'test' ? 'validation' : cargoArgs.includes('--release') || cargoArgs.includes('-r') || cargoArgs[0] === 'bench' ? 'release' : 'dev'
    : undefined);
  if (rustCache) {
    const targetArg = cargoArgs.indexOf('--target-dir');
    const suppliedTarget = cargoCommand
      ? (targetArg >= 0 ? cargoArgs[targetArg + 1] : cargoArgs.find(value => value.startsWith('--target-dir='))?.slice('--target-dir='.length))
      : undefined;
    const targetDir = suppliedTarget
      ? resolve(options.cwd ?? process.cwd(), suppliedTarget)
      : resolve(workspaceRoot, process.env.CARGO_TARGET_DIR ?? (rustCache === 'validation' ? 'target/cargo/validation' : 'target/cargo'));
    const commandArgs = [...args];
    if (suppliedTarget && targetArg >= 0) commandArgs[targetArg + 1] = targetDir;
    else if (suppliedTarget) commandArgs[commandArgs.findIndex(value => value.startsWith('--target-dir='))] = `--target-dir=${targetDir}`;
    const profileArg = cargoArgs.indexOf('--profile');
    const suppliedProfile = cargoCommand
      ? (profileArg >= 0 ? cargoArgs[profileArg + 1] : cargoArgs.find(value => value.startsWith('--profile='))?.slice('--profile='.length))
      : undefined;
    const profile = suppliedProfile ?? (cargoArgs.includes('--release') || cargoArgs.includes('-r') || cargoArgs[0] === 'bench' || rustCache === 'release' ? 'release' : 'dev');
    return withRustCacheLease({ workspaceRoot, targetDir, profile, monitor: options.rustCacheMonitor }, lease =>
      executeTask(command, commandArgs, options, workspaceRoot, lease.env, lease.signal));
  }
  return executeTask(command, args, options, workspaceRoot);
}

async function executeTask(
  command: string,
  args: string[],
  options: TaskOptions,
  workspaceRoot: string,
  cargoEnv: Record<string, string> = {},
  cacheSignal?: AbortSignal,
): Promise<number> {
  cacheSignal?.throwIfAborted();
  const context: TaskContext = {
    workspaceRoot,
    id: randomUUID(),
  };
  const env = prepareTask(context);
  let signalCode: number | undefined;
  const child = spawnManagedProcess({
    name: command,
    command,
    args,
    env: { ...env, ...cargoEnv },
    cwd: options.cwd,
  });
  let stopping: Promise<void> | undefined;
  const stop = (code: number) => {
    signalCode = code;
    stopping ??= stopManagedProcess(child);
  };
  const onInterrupt = () => {
    stop(130);
  };
  const onTerminate = () => {
    stop(143);
  };
  const onCacheAbort = () => stop(1);
  process.once('SIGINT', onInterrupt);
  process.once('SIGTERM', onTerminate);

  try {
    return await new Promise<number>((resolveExit, reject) => {
      child.once('error', reject);
      child.once('exit', (code, signal) => {
        resolveExit(signalCode ?? code ?? (signal === 'SIGINT' ? 130 : 1));
      });
      cacheSignal?.addEventListener('abort', onCacheAbort, { once: true });
      if (cacheSignal?.aborted) onCacheAbort();
    });
  } finally {
    cacheSignal?.removeEventListener('abort', onCacheAbort);
    process.removeListener('SIGINT', onInterrupt);
    process.removeListener('SIGTERM', onTerminate);
    await stopping;
    cleanupTask(context);
  }
}

import { randomUUID } from 'node:crypto';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

import { spawnManagedProcess, stopManagedProcess } from './process.js';

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
}

/** Run one command with owned scratch, preserving its exit status. */
export async function runTask(
  command: string,
  args: string[],
  options: TaskOptions = {},
): Promise<number> {
  const context: TaskContext = {
    workspaceRoot: fileURLToPath(new URL('../../..', import.meta.url)),
    id: randomUUID(),
  };
  const env = prepareTask(context);
  let signalCode: number | undefined;
  const child = spawnManagedProcess({
    name: command,
    command,
    args,
    env,
    cwd: options.cwd,
  });
  const onInterrupt = () => {
    signalCode = 130;
    void stopManagedProcess(child);
  };
  const onTerminate = () => {
    signalCode = 143;
    void stopManagedProcess(child);
  };
  process.once('SIGINT', onInterrupt);
  process.once('SIGTERM', onTerminate);

  try {
    return await new Promise<number>((resolve, reject) => {
      child.once('error', reject);
      child.once('exit', (code, signal) => {
        resolve(signalCode ?? code ?? (signal === 'SIGINT' ? 130 : 1));
      });
    });
  } finally {
    process.removeListener('SIGINT', onInterrupt);
    process.removeListener('SIGTERM', onTerminate);
    cleanupTask(context);
  }
}

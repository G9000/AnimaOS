import type { ManagedProcessDefinition } from '../../../tools/workspace-dev/src/process.js';
import { resolve } from 'node:path';

export const villageDefaults = {
  apiPort: 28192,
  webPort: 24302,
  apiOrigin: 'http://127.0.0.1:28192',
};
function port(value: string | undefined, fallback: number): number {
  const parsed = value === undefined ? fallback : Number(value);
  if (!Number.isInteger(parsed) || parsed < 1024 || parsed > 65535)
    throw new Error('Village port must be an integer from 1024 to 65535.');
  return parsed;
}
export function villagePorts(env: NodeJS.ProcessEnv) {
  const apiPort = port(env.VILLAGE_API_PORT, villageDefaults.apiPort);
  const webPort = port(env.VILLAGE_WEB_PORT, villageDefaults.webPort);
  if (apiPort === webPort)
    throw new Error('Village API and browser must use different ports.');
  return { apiPort, webPort, apiOrigin: `http://127.0.0.1:${apiPort}` };
}
export function villageCargoTarget(root: string, env: NodeJS.ProcessEnv, mode: 'dev' | 'validation' = 'dev'): string {
  return resolve(root, '../..', env.CARGO_TARGET_DIR ?? (mode === 'validation' ? 'target/cargo/validation' : 'target/cargo'));
}
export function villageProcessDefinitions(
  root: string,
  env: NodeJS.ProcessEnv,
): ManagedProcessDefinition[] {
  const { apiPort, webPort, apiOrigin } = villagePorts(env);
  return [
    {
      name: 'Village Rust game host',
      command: 'bun',
      args: [
        resolve(root, '../../tools/workspace-dev/scripts/run-task.ts'),
        '--rust-cache-runtime',
        'dev',
        '--',
        'bun',
        resolve(root, 'tools/rust-task.ts'),
        'run',
      ],
      cwd: root,
      env: {
        CARGO_TARGET_DIR: villageCargoTarget(root, env),
        VILLAGE_BIND: `127.0.0.1:${apiPort}`,
        VILLAGE_DB_PATH:
          env.VILLAGE_DB_PATH ??
          resolve(root, '.anima-village/village.sqlite3'),
      },
    },
    {
      name: 'Village browser',
      command: 'bun',
      args: ['run', 'dev:web'],
      cwd: root,
      env: {
        VILLAGE_API_ORIGIN: apiOrigin,
        VILLAGE_WEB_PORT: String(webPort),
      },
    },
  ];
}

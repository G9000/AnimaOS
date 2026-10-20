import { homedir } from 'node:os';
import { join } from 'node:path';

export const HOST_KEYS = ['rust'] as const;

export type HostKey = (typeof HOST_KEYS)[number];

export interface HostDefinition {
  key: HostKey;
  projectName: string;
  baseUrl: string;
  env: Record<string, string>;
}

const HOSTS: Record<HostKey, HostDefinition> = {
  rust: {
    key: 'rust',
    projectName: 'rust-daemon',
    baseUrl: 'http://127.0.0.1:8080',
    env: {
      ANIMAOS_RS_HOST: '127.0.0.1',
      ANIMAOS_RS_PORT: '8080',
    },
  },
};

export function isHostKey(value: string): value is HostKey {
  return HOST_KEYS.includes(value as HostKey);
}

export function getHostDefinition(value: string): HostDefinition {
  if (!isHostKey(value)) {
    throw new Error(
      `Unknown host '${value}'. Expected one of: ${HOST_KEYS.join(', ')}.`
    );
  }

  return HOSTS[value];
}

export const CONTROL_PLANE_FILE_ENV = 'ANIMAOS_RS_CONTROL_PLANE_FILE';
export const DEV_EPHEMERAL_ENV = 'ANIMAOS_DEV_EPHEMERAL';

/** The daemon keeps memory.sqlite and history.sqlite beside this file. */
export function defaultControlPlaneFile(homeDir: string): string {
  return join(homeDir, '.animaos-swarm', 'data', 'control-plane.json');
}

/**
 * Without a control-plane file the rust daemon forgets agents, connectors,
 * history and memories on every restart, so dev persists by default. A path
 * or Postgres mode from the shell wins, and ANIMAOS_DEV_EPHEMERAL=1 opts out.
 */
export function hostProcessEnv(
  host: HostDefinition,
  env: NodeJS.ProcessEnv = process.env,
  homeDir: string = homedir()
): Record<string, string> {
  const keepsOwnStore =
    env[CONTROL_PLANE_FILE_ENV] !== undefined ||
    env.ANIMAOS_RS_PERSISTENCE_MODE?.toLowerCase() === 'postgres' ||
    env[DEV_EPHEMERAL_ENV] === '1';

  if (keepsOwnStore) {
    return host.env;
  }

  return {
    ...host.env,
    [CONTROL_PLANE_FILE_ENV]: defaultControlPlaneFile(homeDir),
  };
}

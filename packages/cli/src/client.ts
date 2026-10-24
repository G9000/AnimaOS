import {
  type AgentRunResponse,
  type AgentSnapshot,
  type DaemonClient,
  type DaemonClientOptions,
  DaemonHttpError,
  type SwarmRunResponse,
  createDaemonClient,
} from '@animaos/sdk';
import { readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

export type CliDaemonClient = DaemonClient;

export function createCliDaemonClient(
  options: DaemonClientOptions = {}
): CliDaemonClient {
  const baseUrl = options.baseUrl ?? process.env.ANIMA_DAEMON_URL;
  const ownerToken = options.ownerToken ?? loadOwnerToken(process.env, baseUrl);
  return createDaemonClient({
    ...options,
    baseUrl,
    ownerToken,
    apiKey: options.apiKey ?? process.env.ANIMAOS_RS_API_KEY,
  });
}

function loadOwnerToken(
  env: NodeJS.ProcessEnv,
  baseUrl: string | undefined,
): string | undefined {
  if (env.ANIMA_LOCAL_ADMIN_TOKEN_FILE) {
    return readTokenFile(env.ANIMA_LOCAL_ADMIN_TOKEN_FILE, true);
  }
  if (env.ANIMA_LOCAL_ADMIN_TOKEN?.trim()) {
    return env.ANIMA_LOCAL_ADMIN_TOKEN.trim();
  }
  if (!isLoopbackUrl(baseUrl ?? 'http://127.0.0.1:8080')) {
    return undefined;
  }

  const tokenPath = env.ANIMAOS_RS_CONTROL_PLANE_FILE
    ? `${env.ANIMAOS_RS_CONTROL_PLANE_FILE}.admin-token`
    : env.ANIMA_HOME
      ? join(env.ANIMA_HOME, '.animaos-swarm', 'admin-token')
      : `${join(homedir(), '.animaos-swarm', 'data', 'control-plane.json')}.admin-token`;
  return readTokenFile(tokenPath, false);
}

function readTokenFile(filePath: string, required: boolean): string | undefined {
  try {
    const token = readFileSync(filePath, 'utf8').trim();
    return token || undefined;
  } catch (error) {
    if (!required && isMissingFile(error)) return undefined;
    // Never include file contents or raw errors in a CLI message.
    throw new Error(
      'Unable to read the local owner token file; check ANIMA_LOCAL_ADMIN_TOKEN_FILE.',
    );
  }
}

function isMissingFile(error: unknown): boolean {
  return Boolean(error && typeof error === 'object' && 'code' in error && error.code === 'ENOENT');
}

function isLoopbackUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return (
      (url.protocol === 'http:' || url.protocol === 'https:') &&
      ['localhost', '127.0.0.1', '[::1]', '::1'].includes(url.hostname.toLowerCase())
    );
  } catch {
    return false;
  }
}

export { DaemonHttpError };
export type {
  AgentRunResponse,
  AgentSnapshot,
  DaemonClientOptions,
  SwarmRunResponse,
};

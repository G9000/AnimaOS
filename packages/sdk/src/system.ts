import type { DaemonClient } from './client.js';

export type SystemAction =
  | 'start'
  | 'stop'
  | 'restart'
  | 'quit'
  | 'update'
  | 'check-update'
  | 'cancel';

/** The work a drain waits for, by kind. */
export interface SystemInFlight {
  chatRuns: number;
  queuedRuns: number;
  swarmRuns: number;
  helperRuns: number;
  jobs: number;
  backgroundProcesses: number;
  appSessions: number;
  acpSessions: number;
}

/** What the server is doing, tagged by `kind`. */
export type SystemState =
  | { kind: 'starting' }
  | { kind: 'running' }
  | { kind: 'draining'; action: SystemAction; inFlight: SystemInFlight }
  | { kind: 'stopping' }
  | { kind: 'stopped' }
  | { kind: 'crashed'; message: string; retryInSecs: number }
  | { kind: 'updating'; stage: string };

export interface SystemUpdate {
  /** The newer version that can be installed, if any. */
  available: string | null;
  lastError: string | null;
}

/** The Anima app supervisor's status, as it sends it. */
export interface SupervisorStatusJson {
  version: string;
  state: SystemState;
  serverUrl: string;
  workspace: string | null;
  workspaceProblem: string | null;
  dataDir: string;
  autostart: boolean;
  /** The server's connectors, when it could be asked. */
  connectors: number | null;
  /** The action under way; `cancel` until the server confirms its drain ended. */
  pending: SystemAction | null;
  update: SystemUpdate;
}

/** `{ supervised: false }` when the daemon runs without the Anima app. */
export type SystemStatus =
  | { supervised: false; version: string }
  | ({ supervised?: true } & SupervisorStatusJson);

export interface SystemActionOptions {
  /** Skip waiting for running work to finish. */
  force?: boolean;
}

export class SystemClient {
  constructor(private readonly client: DaemonClient) {}

  status(): Promise<SystemStatus> {
    return this.client.requestJson('/api/system', {
      headers: { 'cache-control': 'no-store' },
    });
  }

  action(
    action: SystemAction,
    options: SystemActionOptions = {},
  ): Promise<SystemStatus> {
    return this.client.requestJson(`/api/system/actions/${action}`, {
      method: 'POST',
      headers: { 'cache-control': 'no-store' },
      ...(options.force === undefined
        ? {}
        : { body: { force: options.force } }),
    });
  }
}

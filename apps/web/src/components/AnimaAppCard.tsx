import { useCallback, useEffect, useRef, useState } from 'react';
import {
  DaemonConnectionError,
  DaemonHttpError,
  type SupervisorStatusJson,
  type SystemAction,
  type SystemActionOptions,
  type SystemInFlight,
  type SystemStatus,
} from '@animaos/sdk';

import { systemApi } from '../lib/daemon-api';
import { RevealedText } from './memory/RevealedText';

export const ANIMA_APP_POLL_MS = 2_000;
export const ANIMA_STOPPED =
  'Anima is stopped. Start it from the tray or run `anima start`.';

export interface AnimaAppClient {
  status(): Promise<SystemStatus>;
  action(
    action: SystemAction,
    options?: SystemActionOptions,
  ): Promise<SystemStatus>;
}

const IN_FLIGHT_LABELS: [keyof SystemInFlight, string][] = [
  ['chatRuns', 'Agent runs'],
  ['queuedRuns', 'Queued messages'],
  ['swarmRuns', 'Swarm runs'],
  ['helperRuns', 'Helper runs'],
  ['jobs', 'Jobs'],
  ['backgroundProcesses', 'Background processes'],
  ['appSessions', 'App sessions'],
  ['acpSessions', 'Coding-agent sessions'],
  ['memoryReviews', 'Learning reviews'],
  ['skillEvaluations', 'Skill evaluations'],
  ['taskRegressions', 'Task comparisons'],
  ['doctorRepairs', 'Repairs'],
  ['standingIntentChanges', 'Saved instruction changes'],
  ['executionJobs', 'Commands'],
  ['mcpOperations', 'Connected tool operations'],
  ['browserProfileLogins', 'Browser sign-ins'],
];

const NOW_LABELS: Partial<Record<SystemAction, string>> = {
  restart: 'Restart now',
  update: 'Update now',
  quit: 'Quit now',
};

const AFTER_DRAIN: Partial<Record<SystemAction, string>> = {
  stop: 'Anima will stop',
  quit: 'Anima will quit',
  restart: 'Anima will restart',
  update: 'Anima will update',
};

/** Stop and Quit end the server; the console then cannot reach it. */
const ENDS_SERVER: ReadonlySet<SystemAction> = new Set(['stop', 'quit']);

const withV = (version: string) =>
  version.startsWith('v') ? version : `v${version}`;

const total = (inFlight: SystemInFlight) =>
  IN_FLIGHT_LABELS.reduce((sum, [key]) => sum + (inFlight[key] ?? 0), 0);

function stateLabel(status: SupervisorStatusJson): string {
  const { state } = status;
  switch (state.kind) {
    case 'starting':
      return 'Starting';
    case 'running':
      return 'Running';
    case 'draining':
      return 'Finishing work';
    case 'stopping':
      return 'Stopping';
    case 'stopped':
      return 'Stopped';
    case 'crashed':
      return 'Crashed';
    case 'updating':
      return `Updating (${state.stage})`;
  }
}

function errorText(cause: unknown): string {
  return cause instanceof Error ? cause.message : 'Something went wrong.';
}

/** The Anima app: its version, state and folders, and
 *  the lifecycle buttons. Renders nothing for a daemon without `/api/system`. */
export function AnimaAppCard({
  client = systemApi,
}: {
  client?: AnimaAppClient;
}) {
  const [status, setStatus] = useState<SystemStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [stopped, setStopped] = useState(false);
  const [unavailable, setUnavailable] = useState(false);
  const [reconnecting, setReconnecting] = useState(false);
  // The action the console last sent that takes the server away for a while.
  const away = useRef<SystemAction | null>(null);
  const sequence = useRef(0);
  // Whether any status has been read: an unreachable daemon is the page's news.
  const seen = useRef(false);
  const unmounted = useRef(false);

  const apply = useCallback((next: SystemStatus) => {
    seen.current = true;
    setStatus(next);
    setError(null);
    setUnavailable(false);
    if (next.supervised === false) return;
    const { kind } = next.state;
    if (kind === 'stopped' || (kind === 'stopping' && away.current)) {
      setStopped(true);
      return;
    }
    // The server answers again: whatever was stopping is over.
    setStopped(false);
    setReconnecting(false);
    if (kind === 'running' && next.pending === null) away.current = null;
  }, []);

  const load = useCallback(async () => {
    const mine = ++sequence.current;
    try {
      const next = await client.status();
      if (unmounted.current || mine !== sequence.current) return;
      apply(next);
    } catch (cause) {
      if (unmounted.current || mine !== sequence.current) return;
      if (away.current && ENDS_SERVER.has(away.current)) {
        setStopped(true);
      } else if (away.current) {
        setReconnecting(true);
      } else if (cause instanceof DaemonHttpError && cause.status === 404) {
        setUnavailable(true);
      } else if (seen.current || cause instanceof DaemonHttpError) {
        setError(errorText(cause));
      }
      // Otherwise the daemon is unreachable and the page says so itself.
    }
  }, [client, apply]);

  useEffect(() => {
    unmounted.current = false;
    // The status request settles before load applies any state changes.
    // eslint-disable-next-line react/set-state-in-effect
    void load();
    const timer = setInterval(() => void load(), ANIMA_APP_POLL_MS);
    return () => {
      unmounted.current = true;
      clearInterval(timer);
    };
  }, [load]);

  async function run(action: SystemAction, options?: SystemActionOptions) {
    // A newer answer than any read that is still on its way.
    sequence.current += 1;
    setBusy(true);
    setError(null);
    try {
      const next = await (options
        ? client.action(action, options)
        : client.action(action));
      if (unmounted.current) return;
      if (ENDS_SERVER.has(action) || action === 'restart') {
        away.current = action;
      }
      apply(next);
      if (ENDS_SERVER.has(action) && next.supervised !== false) {
        const settled =
          next.state.kind !== 'draining' && next.pending !== action;
        if (settled) setStopped(true);
      }
    } catch (cause) {
      if (unmounted.current) return;
      if (ENDS_SERVER.has(action) && cause instanceof DaemonConnectionError) {
        // The server went away before it could answer.
        away.current = action;
        setStopped(true);
      } else {
        setError(errorText(cause));
      }
    } finally {
      if (!unmounted.current) setBusy(false);
    }
  }

  if (stopped) {
    return (
      <section className="system-card anima-app-card" aria-label="Anima app">
        <p className="system-card-summary" role="status">
          {ANIMA_STOPPED}
        </p>
      </section>
    );
  }

  if (!status) {
    if (unavailable || !error) return null;
    return (
      <section className="system-card anima-app-card" aria-label="Anima app">
        <p className="system-error" role="alert">
          {error}
        </p>
      </section>
    );
  }

  if (status.supervised === false) {
    return (
      <section className="system-card anima-app-card" aria-label="Anima app">
        <p className="system-card-summary">
          {withV(status.version)} (running without the Anima app)
        </p>
      </section>
    );
  }

  const { state } = status;
  const settling =
    state.kind === 'starting' ||
    state.kind === 'stopping' ||
    state.kind === 'updating';
  const idle = !busy && !settling;
  const cancelling = status.pending === 'cancel';

  return (
    <section className="system-card anima-app-card" aria-label="Anima app">
      <div className="system-card-head">
        <h3>Anima app</h3>
        <span className="system-card-state">{stateLabel(status)}</span>
      </div>
      <dl className="anima-app-facts">
        <dt>Version</dt>
        <dd>{withV(status.version)}</dd>
        <dt>Workspace</dt>
        <dd>{status.workspace ?? 'Not set'}</dd>
        <dt>Data folder</dt>
        <dd>{status.dataDir}</dd>
        <dt>Start at login</dt>
        <dd>{status.autostart ? 'On' : 'Off'}</dd>
      </dl>
      {status.workspaceProblem && (
        <p className="system-error">
          <RevealedText text={status.workspaceProblem} />
        </p>
      )}
      {state.kind === 'crashed' && (
        <p className="system-error">
          <RevealedText text={state.message} /> Starting again in{' '}
          {state.retryInSecs} seconds.
        </p>
      )}
      {status.update.available && (
        <p className="system-note">
          Update available: {withV(status.update.available)}
        </p>
      )}
      {status.update.lastError && (
        <p className="system-note">
          Update problem: <RevealedText text={status.update.lastError} />
        </p>
      )}
      {reconnecting && (
        <p className="system-note" role="status">
          Waiting for Anima to come back…
        </p>
      )}
      {state.kind === 'draining' ? (
        <div className="anima-app-drain">
          <p className="system-card-summary" role="status">
            {total(state.inFlight) === 0
              ? 'Finishing up'
              : `Waiting for ${total(state.inFlight)} ${
                  total(state.inFlight) === 1 ? 'task' : 'tasks'
                } to finish`}
          </p>
          <ul className="system-card-details">
            {IN_FLIGHT_LABELS.filter(
              ([key]) => (state.inFlight[key] ?? 0) > 0,
            ).map(([key, label]) => (
              <li key={key}>
                {label}: {state.inFlight[key]}
              </li>
            ))}
          </ul>
          {AFTER_DRAIN[state.action] && (
            <p className="system-note">{AFTER_DRAIN[state.action]}.</p>
          )}
          <div className="anima-app-actions">
            <button
              type="button"
              className="studio-tool-button"
              disabled={busy}
              onClick={() => void run(state.action, { force: true })}
            >
              {NOW_LABELS[state.action] ?? 'Stop now'}
            </button>
            {cancelling ? (
              <span className="system-note" role="status">
                Cancelling…
              </span>
            ) : (
              <button
                type="button"
                className="studio-tool-button"
                disabled={busy}
                onClick={() => void run('cancel')}
              >
                Cancel
              </button>
            )}
          </div>
        </div>
      ) : (
        <div className="anima-app-actions">
          {status.update.available ? (
            <button
              type="button"
              className="studio-tool-button"
              disabled={!idle}
              onClick={() => void run('update')}
            >
              Update to {withV(status.update.available)}
            </button>
          ) : (
            <button
              type="button"
              className="studio-tool-button"
              disabled={!idle}
              onClick={() => void run('check-update')}
            >
              Check for updates
            </button>
          )}
          <button
            type="button"
            className="studio-tool-button"
            disabled={!idle}
            onClick={() => void run('restart')}
          >
            Restart
          </button>
          <button
            type="button"
            className="studio-tool-button"
            disabled={!idle}
            onClick={() => void run('stop')}
          >
            Stop
          </button>
          {cancelling && (
            <span className="system-note" role="status">
              Cancelling…
            </span>
          )}
        </div>
      )}
      {error && (
        <p className="system-error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

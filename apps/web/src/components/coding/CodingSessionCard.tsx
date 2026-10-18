import { useEffect, useState } from 'react';

import {
  formatClock,
  shortPath,
  type AcpSessionView,
  type AcpStep,
  type AppSessionView,
  type CodingSessionView,
} from '../../lib/coding-sessions';
import { daemon } from '../../lib/daemon-api';

const KNOWN_APPS: Record<string, string> = {
  claude: 'Claude Code',
  'claude-code-acp': 'Claude Code',
  codex: 'Codex',
  'codex-acp': 'Codex',
  opencode: 'OpenCode',
  gemini: 'Gemini CLI',
};

/** "Claude Code" for `claude -p …`, else the program's own name. */
export function appDisplayName(command: string): string {
  const program = command.trim().split(/\s+/)[0] ?? '';
  const base = program.split(/[\\/]/).at(-1) ?? program;
  return KNOWN_APPS[base.replace(/\.exe$/i, '')] ?? (base || 'App');
}

const OUTCOMES: Record<string, string> = {
  matched: 'Found what it was waiting for',
  settled: 'The screen settled',
  exited: 'The app exited',
  timed_out: 'Stopped waiting',
};

function errorText(caught: unknown): string {
  return caught instanceof Error ? caught.message : String(caught);
}

function useNow(ticking: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!ticking) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [ticking]);
  return now;
}

/** Stop / Cancel turn: one request at a time, the outcome kept locally. */
function useSessionAction() {
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const run = async (name: string, action: () => Promise<unknown>) => {
    if (busy) return false;
    setBusy(name);
    setError(null);
    try {
      await action();
      return true;
    } catch (caught) {
      setError(errorText(caught));
      return false;
    } finally {
      setBusy(null);
    }
  };
  return { busy, error, run };
}

function StatusBadge({
  tone,
  label,
}: {
  tone: 'live' | 'wait' | 'done';
  label: string;
}) {
  return (
    <span className="coding-session-status" data-tone={tone}>
      <span className="coding-session-dot" aria-hidden="true" />
      {label}
    </span>
  );
}

function AppSessionCard({
  session,
  agentId,
  active,
  footer,
}: {
  session: AppSessionView;
  agentId?: string;
  active: boolean;
  footer?: string;
}) {
  const [stopped, setStopped] = useState(false);
  const [full, setFull] = useState(false);
  const action = useSessionAction();
  const status = stopped ? 'stopped' : session.status;
  const running = status === 'running';
  const now = useNow(running && active);
  const name = appDisplayName(session.command);
  const label =
    status === 'running'
      ? `RUNNING${active && session.startedAtMs !== null ? ` · ${formatClock(now - session.startedAtMs)}` : ''}`
      : status === 'exited'
        ? `EXITED${session.exitCode !== null ? ` · ${session.exitCode}` : ''}`
        : 'STOPPED';
  const screen =
    session.rows.length > 0 ? session.rows.join('\n') : session.output.trim();
  const notes = [
    footer,
    session.outcome ? OUTCOMES[session.outcome] : null,
  ].filter((note): note is string => !!note);
  return (
    <section
      className="coding-session"
      data-kind="app"
      aria-label={`${name} app session`}
    >
      <header className="coding-session-head">
        <span className="coding-session-icon" aria-hidden="true">
          ▣
        </span>
        <strong className="coding-session-name">{name}</strong>
        <span className="coding-session-meta">
          {[session.command, shortPath(session.cwd)].filter(Boolean).join(' · ')}
        </span>
        <StatusBadge tone={running ? 'live' : 'done'} label={label} />
        {agentId && running && (
          <button
            type="button"
            className="coding-session-button"
            disabled={action.busy !== null}
            onClick={() =>
              void action
                .run('stop', () => daemon.stopAppSession(agentId, session.id))
                .then((ok) => ok && setStopped(true))
            }
          >
            <span aria-hidden="true">□</span>{' '}
            {action.busy === 'stop' ? 'Stopping…' : 'Stop'}
          </button>
        )}
      </header>
      {/* The app wrote these: a text node, never markup. */}
      <pre
        className="coding-session-screen"
        data-full={full || undefined}
        aria-label={`${name} screen`}
      >
        {screen || 'Nothing on screen yet.'}
      </pre>
      {action.error && (
        <p className="coding-session-error" role="alert">
          {action.error}
        </p>
      )}
      <footer className="coding-session-foot">
        <span>{notes.join(' · ')}</span>
        <button
          type="button"
          className="coding-session-link"
          aria-pressed={full}
          onClick={() => setFull((value) => !value)}
        >
          {full ? 'Less' : 'Full screen'}
        </button>
      </footer>
    </section>
  );
}

const STEP_LABELS: Record<AcpStep['status'], string> = {
  pending: 'Waiting',
  in_progress: 'Working',
  completed: 'Done',
  failed: 'Failed',
};

function stepLabel(step: AcpStep): string {
  if (step.decision === 'allowed')
    return step.status === 'completed' ? 'Allowed · done' : 'Allowed';
  if (step.decision === 'denied') return 'Denied';
  if (step.decision === 'cancelled') return 'Cancelled';
  return STEP_LABELS[step.status];
}

const PLAN_ICONS = { completed: '✓', in_progress: '◐', pending: '○' };

function AcpSessionCard({
  session,
  agentId,
}: {
  session: AcpSessionView;
  agentId?: string;
}) {
  const [local, setLocal] = useState<'stopped' | 'idle' | null>(null);
  const action = useSessionAction();
  const status = local ?? session.status;
  const name =
    session.agentName ?? appDisplayName(session.command) ?? 'Coding agent';
  const ended = status === 'exited' || status === 'stopped';
  const turnLive = status === 'running' || status === 'waiting_for_you';
  const label = status === 'waiting_for_you' ? 'WAITING FOR YOU' : status.toUpperCase();
  const waiting = local ? null : session.waitingOn;
  return (
    <section
      className="coding-session"
      data-kind="acp"
      aria-label={`${name} coding agent`}
    >
      <header className="coding-session-head">
        <span className="coding-session-icon" aria-hidden="true">
          ◫
        </span>
        <strong className="coding-session-name">{name}</strong>
        <span className="coding-session-meta">
          {['ACP', session.command, shortPath(session.cwd)]
            .filter(Boolean)
            .join(' · ')}
        </span>
        <StatusBadge
          tone={
            status === 'waiting_for_you'
              ? 'wait'
              : status === 'running'
                ? 'live'
                : 'done'
          }
          label={label}
        />
        {agentId && turnLive && (
          <button
            type="button"
            className="coding-session-button"
            disabled={action.busy !== null}
            onClick={() =>
              void action
                .run('cancel', () => daemon.cancelAcpTurn(agentId, session.id))
                .then((ok) => ok && setLocal('idle'))
            }
          >
            {action.busy === 'cancel' ? 'Cancelling…' : 'Cancel turn'}
          </button>
        )}
        {agentId && !ended && (
          <button
            type="button"
            className="coding-session-button"
            disabled={action.busy !== null}
            onClick={() =>
              void action
                .run('stop', () => daemon.stopAcpSession(agentId, session.id))
                .then((ok) => ok && setLocal('stopped'))
            }
          >
            <span aria-hidden="true">□</span>{' '}
            {action.busy === 'stop' ? 'Stopping…' : 'Stop'}
          </button>
        )}
      </header>
      {session.plan.length > 0 && (
        <div className="coding-session-part">
          <p className="coding-session-kicker">Plan</p>
          <ul className="coding-session-plan" aria-label="Plan">
            {session.plan.map((entry, index) => (
              <li key={`${index}:${entry.content}`} data-status={entry.status}>
                <span aria-hidden="true">{PLAN_ICONS[entry.status]}</span>
                <span>{entry.content}</span>
                <span className="sr-only">
                  , {entry.status.replace('_', ' ')}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}
      {session.steps.length > 0 && (
        <div className="coding-session-part">
          <p className="coding-session-kicker">Steps</p>
          <ul className="coding-session-steps" aria-label="Steps">
            {session.steps.map((step) => (
              <li key={step.id} data-status={step.status}>
                <span className="coding-session-step-title">{step.title}</span>
                <span className="coding-session-step-state">
                  {stepLabel(step)}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}
      {session.message && (
        <p className="coding-session-message">{session.message}</p>
      )}
      {waiting && (
        <div className="coding-session-waiting" role="status">
          <strong>
            {name} wants to {waiting.title.charAt(0).toLowerCase()}
            {waiting.title.slice(1)}
          </strong>
          <span>It waits until you answer the approval below.</span>
        </div>
      )}
      {action.error && (
        <p className="coding-session-error" role="alert">
          {action.error}
        </p>
      )}
      {ended && session.stopReason && (
        <footer className="coding-session-foot">
          <span>Last turn ended: {session.stopReason.replace(/_/g, ' ')}</span>
        </footer>
      )}
    </section>
  );
}

/** An app or coding-agent session as the chat shows it. */
export function CodingSessionCard({
  session,
  agentId,
  active = false,
  footer,
}: {
  session: CodingSessionView;
  /** The agent that owns the session; without it there is no Stop. */
  agentId?: string;
  /** The run is still working, so a running clock ticks. */
  active?: boolean;
  footer?: string;
}) {
  return session.kind === 'app_session' ? (
    <AppSessionCard
      session={session}
      agentId={agentId}
      active={active}
      footer={footer}
    />
  ) : (
    <AcpSessionCard session={session} agentId={agentId} />
  );
}

import { useEffect, useRef, useState } from 'react';
import type { AgentMemory, Session } from '@animaOS-SWARM/sdk';

import { daemon } from '../../lib/daemon-api';
import {
  buildWeeklyReport,
  formatMicros,
  reportWindow,
  type WeeklyReport as Report,
} from '../../lib/weekly-report';
import { XIcon } from '../icons';

/** Sessions read for the report: the most recently active ones. */
const MAX_REPORT_SESSIONS = 40;
/** The daemon's largest page of runs (`MAX_RUN_PAGE`): each session's
 *  latest runs, which covers a week for all but the busiest chats. */
const RUN_PAGE = 50;

type Load =
  | { state: 'loading' }
  | { state: 'error' }
  | { state: 'ready'; report: Report };

function range(from: number, to: number): string {
  const format = (ms: number) =>
    new Date(ms).toLocaleDateString([], { month: 'short', day: 'numeric' });
  return `${format(from)} – ${format(to)}`;
}

/** The weekly report: what the agent did this week, what it cost, what went
 *  wrong and what it kept about the owner, from the daemon's records only. */
export function WeeklyReport({
  agentId,
  agentName,
  sessions,
  openSession,
  onClose,
}: {
  agentId: string;
  agentName: string;
  sessions: readonly Session[];
  openSession: (sessionId: string) => void;
  onClose: () => void;
}) {
  const [load, setLoad] = useState<Load>({ state: 'loading' });
  const [forgotten, setForgotten] = useState<ReadonlySet<string>>(new Set());
  const [forgetError, setForgetError] = useState<string | null>(null);
  const dialog = useRef<HTMLDivElement>(null);
  const sessionsRef = useRef(sessions);
  sessionsRef.current = sessions;

  useEffect(() => {
    dialog.current?.focus();
    const escape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', escape);
    return () => document.removeEventListener('keydown', escape);
  }, [onClose]);

  useEffect(() => {
    let current = true;
    const { from, to } = reportWindow();
    const recent = sessionsRef.current
      .filter((session) => session.lastActivityAtMs >= from)
      .sort((left, right) => right.lastActivityAtMs - left.lastActivityAtMs)
      .slice(0, MAX_REPORT_SESSIONS);
    Promise.all([
      Promise.all(
        recent.map((session) =>
          daemon.sessionRuns(session.agentId, session.id, { limit: RUN_PAGE }),
        ),
      ).then((runs) => runs.flat()),
      daemon
        .usageSummary({
          from,
          to,
          groupBy: 'day',
          tzOffsetMinutes: -new Date().getTimezoneOffset(),
        })
        .catch(() => null),
      daemon
        .recentAgentMemories(agentId)
        .then(({ memories }) => memories)
        .catch((): AgentMemory[] => []),
    ])
      .then(([runs, usage, memories]) => {
        if (current)
          setLoad({
            state: 'ready',
            report: buildWeeklyReport({ runs, usage, memories, from, to }),
          });
      })
      .catch(() => {
        if (current) setLoad({ state: 'error' });
      });
    return () => {
      current = false;
    };
  }, [agentId]);

  const forget = async (memory: AgentMemory) => {
    setForgetError(null);
    try {
      await daemon.deleteMemory(memory.id, memory.content);
      setForgotten((current) => new Set(current).add(memory.id));
    } catch {
      setForgetError('Could not forget that. Try again from Memory.');
    }
  };

  return (
    <div className="weekly-report-scrim" onPointerDown={(event) => {
      if (event.target === event.currentTarget) onClose();
    }}>
      <div
        ref={dialog}
        className="weekly-report"
        role="dialog"
        aria-modal="true"
        aria-label="Weekly report"
        tabIndex={-1}
      >
        <header className="weekly-report-head">
          <div>
            <p className="weekly-report-kicker">
              Weekly report
              {load.state === 'ready' &&
                ` · ${range(load.report.from, load.report.to)}`}
            </p>
            <h2>
              {load.state === 'ready'
                ? headline(load.report, agentName)
                : `${agentName}’s week`}
            </h2>
          </div>
          <button
            type="button"
            className="studio-tool-button"
            aria-label="Close weekly report"
            onClick={onClose}
          >
            <XIcon size={15} />
          </button>
        </header>

        {load.state === 'loading' ? (
          <p className="weekly-report-empty" role="status">
            Reading this week’s activity…
          </p>
        ) : load.state === 'error' ? (
          <p className="weekly-report-error" role="alert">
            Could not read this week’s activity. Try again when {agentName} is
            reachable.
          </p>
        ) : (
          <div className="weekly-report-body">
            <div className="weekly-report-main">
              <div className="weekly-report-tiles">
                <Tile value={load.report.finished} label="replies and tasks finished" />
                <Tile value={load.report.actions} label="things changed" />
                <Tile
                  value={formatMicros(load.report.spentMicros)}
                  label="spent on models"
                  note={
                    load.report.unpricedCalls > 0
                      ? `${load.report.unpricedCalls} calls not priced`
                      : undefined
                  }
                />
                <Tile
                  value={load.report.failed.length}
                  label="didn’t work"
                  warn={load.report.failed.length > 0}
                />
              </div>

              <section aria-labelledby="report-highlights">
                <h3 id="report-highlights">What it did</h3>
                {load.report.highlights.length === 0 ? (
                  <p className="weekly-report-quiet">
                    Only conversation this week, nothing changed.
                  </p>
                ) : (
                  <ul className="weekly-report-list">
                    {load.report.highlights.map((item) => (
                      <li key={item.title}>
                        <span>{item.title}</span>
                        <span className="weekly-report-count">× {item.count}</span>
                      </li>
                    ))}
                  </ul>
                )}
              </section>

              <section aria-labelledby="report-failed">
                <h3 id="report-failed">What didn’t work</h3>
                {load.report.failed.length === 0 ? (
                  <p className="weekly-report-quiet">Nothing failed this week.</p>
                ) : (
                  <ul className="weekly-report-issues">
                    {load.report.failed.slice(0, 5).map((issue) => (
                      <li key={issue.runId}>
                        <span>{issue.reason}</span>
                        <button
                          type="button"
                          className="studio-tool-button"
                          onClick={() => {
                            openSession(issue.sessionId);
                            onClose();
                          }}
                        >
                          Open chat
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </section>
            </div>

            <aside className="weekly-report-side">
              <section aria-labelledby="report-spend">
                <h3 id="report-spend">Spending by day</h3>
                <Bars days={load.report.days} />
              </section>
              <section aria-labelledby="report-learned">
                <h3 id="report-learned">What it kept about you</h3>
                {load.report.learned.length === 0 ? (
                  <p className="weekly-report-quiet">No new memories this week.</p>
                ) : (
                  <ul className="weekly-report-facts">
                    {load.report.learned.slice(0, 6).map((memory) => (
                      <li key={memory.id}>
                        <span>{memory.content}</span>
                        {forgotten.has(memory.id) ? (
                          <span className="weekly-report-forgotten">Forgotten</span>
                        ) : (
                          <button
                            type="button"
                            aria-label={`Forget: ${memory.content}`}
                            onClick={() => void forget(memory)}
                          >
                            Forget
                          </button>
                        )}
                      </li>
                    ))}
                  </ul>
                )}
                {forgetError && (
                  <p className="weekly-report-error" role="alert">
                    {forgetError}
                  </p>
                )}
              </section>
            </aside>
          </div>
        )}
        <p className="weekly-report-foot">
          From {agentName}’s own records: runs, actions, model usage and memory.
        </p>
      </div>
    </div>
  );
}

function headline(report: Report, agentName: string): string {
  if (report.finished === 0 && report.actions === 0)
    return `A quiet week. ${agentName} is ready when you are.`;
  const failed =
    report.failed.length === 0
      ? 'and nothing failed'
      : `and ${report.failed.length} ${report.failed.length === 1 ? 'thing' : 'things'} to look at`;
  return `${report.finished} finished, ${report.actions} changes, ${failed}.`;
}

function Tile({
  value,
  label,
  note,
  warn = false,
}: {
  value: string | number;
  label: string;
  note?: string;
  warn?: boolean;
}) {
  return (
    <div className="weekly-report-tile" data-warn={warn || undefined}>
      <strong>{value}</strong>
      <span>{label}</span>
      {note && <small>{note}</small>}
    </div>
  );
}

function Bars({ days }: { days: Report['days'] }) {
  const max = Math.max(...days.map((day) => day.micros), 1);
  return (
    <div
      className="weekly-report-bars"
      role="img"
      aria-label={days
        .map((day) => `${day.label} ${formatMicros(day.micros)}`)
        .join(', ')}
    >
      {days.map((day, index) => (
        <div key={index} className="weekly-report-bar">
          <i
            style={{ height: `${Math.max(2, (day.micros / max) * 100)}%` }}
            data-peak={day.micros === max && day.micros > 0 ? true : undefined}
          />
          <span>{day.label}</span>
        </div>
      ))}
    </div>
  );
}

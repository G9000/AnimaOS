import { useEffect, useState } from 'react';
import type { LogLevel, LogLine as LogLineData } from '@animaos/sdk';
import { AnimaAppCard } from '../components/AnimaAppCard';
import { LogLine } from '../components/system/LogLine';
import { StatusCard } from '../components/system/StatusCard';
import { useStatus } from '../hooks/useStatus';
import { daemon } from '../lib/daemon-api';
import { COMPANION_UNREACHABLE } from '../lib/approvals';
import { HEALTH_LOADING, HEALTH_TOO_OLD, healthCards } from '../lib/status';

export interface HealthPageProps {
  online: boolean;
  /** `LiveState.epoch`: bumped by every snapshot and resync. */
  epoch: number;
}

/** Spec §15.4: whether the daemon is healthy. Daemon text renders as text. */
export function HealthPage({ online, epoch }: HealthPageProps) {
  const view = useStatus({ enabled: online, epoch });
  const [logLevel, setLogLevel] = useState<LogLevel>('info');
  const [recentLogs, setRecentLogs] = useState<LogLineData[]>([]);
  const [reportNotice, setReportNotice] = useState('');

  useEffect(() => {
    if (!online) return;
    let current = true;
    void daemon
      .logs({ level: logLevel, limit: 20 })
      .then((result) => {
        if (current) setRecentLogs(result.lines.slice(-6));
      })
      .catch(() => {
        if (current) setRecentLogs([]);
      });
    return () => {
      current = false;
    };
  }, [online, epoch, logLevel]);

  if (!online) {
    return (
      <div className="system-page">
        <p className="system-note" role="status">
          {COMPANION_UNREACHABLE}
        </p>
      </div>
    );
  }

  const gone = view.errorStatus === 404;
  if (gone) {
    return (
      <div className="system-page">
        <p className="system-note" role="status">
          {HEALTH_TOO_OLD}
        </p>
      </div>
    );
  }

  const copyReport = async () => {
    if (!view.status) return;
    try {
      await navigator.clipboard.writeText(
        JSON.stringify({ status: view.status, recentLogs }, null, 2),
      );
      setReportNotice('Report copied.');
    } catch {
      setReportNotice('Could not copy the report in this browser.');
    }
  };
  const copyDoctorCommand = async () => {
    try {
      await navigator.clipboard.writeText('anima doctor');
      setReportNotice('Command copied. Run it in your terminal.');
    } catch {
      setReportNotice('Could not copy the command in this browser.');
    }
  };

  return (
    <div className="system-page">
      <AnimaAppCard />
      <div className="system-header">
        <h2 className="sr-only">How the daemon is doing</h2>
        <button
          type="button"
          className="studio-tool-button"
          onClick={view.refresh}
        >
          Refresh
        </button>
      </div>
      {view.error && (
        <p className="system-error" role="alert">
          {view.error}
        </p>
      )}
      {!view.loaded && !view.error && (
        <p className="system-note" role="status">
          {HEALTH_LOADING}
        </p>
      )}
      {view.status && (
        <div className="system-cards">
          {healthCards(view.status).map((card) => (
            <StatusCard key={card.id} card={card} />
          ))}
        </div>
      )}
      <div className="health-actions">
        <button
          type="button"
          className="studio-tool-button"
          onClick={() => void copyDoctorCommand()}
        >
          Copy doctor command
        </button>
        <button
          type="button"
          className="studio-tool-button"
          disabled={!view.status}
          onClick={() => void copyReport()}
        >
          Copy report
        </button>
        <a className="studio-tool-button" href="#/logs">
          Open Logs
        </a>
        {reportNotice && (
          <span role="status" className="system-note">
            {reportNotice}
          </span>
        )}
      </div>
      <section className="health-recent-logs" aria-labelledby="health-recent-logs">
        <div className="health-recent-logs-head">
          <h2 id="health-recent-logs">Recent log</h2>
          <div className="health-log-filters" role="group" aria-label="Log level">
            {([
              ['info', 'Info and above'],
              ['warn', 'Warnings'],
              ['error', 'Errors'],
            ] as const).map(([level, label]) => (
              <button
                key={level}
                type="button"
                aria-pressed={logLevel === level}
                onClick={() => setLogLevel(level)}
              >
                {label}
              </button>
            ))}
          </div>
        </div>
        {recentLogs.length ? (
          <div className="system-log" aria-label="Recent daemon logs">
            {recentLogs.map((line) => (
              <LogLine key={line.seq} line={line} />
            ))}
          </div>
        ) : (
          <p className="system-note">No recent log lines at this level.</p>
        )}
      </section>
    </div>
  );
}

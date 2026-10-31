import type { TaskRegressionReport } from '@animaos/sdk';
import { ghostBtnCls } from '../ui-bits';

/** Private owner presentation; never serialize this report into actor messages. */
export function TaskRegressionCard({
  report,
  busy = false,
  onRefresh,
  onCancel,
}: {
  report: TaskRegressionReport;
  busy?: boolean;
  onRefresh: () => void;
  onCancel: () => void;
}) {
  const comparison = report.comparison;
  const failures =
    comparison?.trials.filter((trial) => trial.failures.length > 0) ?? [];
  return (
    <article
      aria-label="Task comparison report"
      className="space-y-3 rounded-xl border border-line bg-surface-2 p-4 text-sm"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <strong>{report.suite.name}</strong>
        <span role="status">{report.status}</span>
      </div>
      <p className="break-all text-xs text-ink-3">Report {report.id}</p>
      <p>
        {report.completedTrials} / {report.plannedTrials} trials completed
      </p>
      {report.evidenceScope === 'runtimePlumbing' ? (
        <p>Runtime plumbing evidence only.</p>
      ) : (
        <p>
          Selected-model tasks with deterministic file and mock-state checks.
        </p>
      )}
      {comparison?.matrixComplete ? (
        <div>
          <p>
            Baseline: {comparison.baseline.correct} /{' '}
            {comparison.baseline.trials} tasks passed
          </p>
          <p>
            Candidate: {comparison.candidate.correct} /{' '}
            {comparison.candidate.trials} tasks passed
          </p>
          <p className="text-xs text-ink-3">Counts include every repetition.</p>
        </div>
      ) : (
        <p>Complete task matrix unavailable; no task pass conclusion.</p>
      )}
      <p>
        {report.usageComplete
          ? 'Provider usage reported.'
          : 'Usage incomplete; remaining provider usage is unknown.'}
      </p>
      {comparison && (
        <p className="text-xs text-ink-3">
          Known tokens — baseline:{' '}
          {comparison.baseline.metrics.knownUsage?.total_tokens ??
            'not reported'}
          ; candidate:{' '}
          {comparison.candidate.metrics.knownUsage?.total_tokens ??
            'not reported'}
          . Unknown-usage calls:{' '}
          {comparison.baseline.metrics.unknownUsageCalls +
            comparison.candidate.metrics.unknownUsageCalls}
          .
        </p>
      )}
      {report.status !== 'running' && report.retiredAtMs === null && (
        <p className="text-xs text-ink-3">
          Provider work may still be settling; cancellation does not establish
          zero usage.
        </p>
      )}
      {!!failures.length && (
        <details>
          <summary className="cursor-pointer">
            Task failures ({failures.length})
          </summary>
          <ul className="mt-2 space-y-2 break-words">
            {failures.slice(0, 20).map((trial) => (
              <li
                key={`${trial.fixtureId}:${trial.repetition}:${trial.variant}`}
              >
                {trial.fixtureId} · repetition {trial.repetition} ·{' '}
                {trial.variant}: {trial.failures.join('; ')}
              </li>
            ))}
          </ul>
          {failures.length > 20 && (
            <p>{failures.length - 20} more failed trials in this report.</p>
          )}
        </details>
      )}
      <details>
        <summary className="cursor-pointer">Pinned versions</summary>
        <dl className="mt-2 space-y-2 break-all text-xs text-ink-3">
          <div>
            <dt>Suite</dt>
            <dd>{report.suite.digest}</dd>
          </div>
          {(['baseline', 'candidate'] as const).map((variant) => (
            <div key={variant}>
              <dt>{variant === 'baseline' ? 'Baseline' : 'Candidate'}</dt>
              <dd>
                {report[variant].provider} / {report[variant].model} ·{' '}
                {report[variant].toolPolicy}
              </dd>
              <dd>
                Selected: {report[variant].agentId} ·{' '}
                {report[variant].configDigest}
              </dd>
              <dd>Effective: {report[variant].effectiveConfigDigest}</dd>
            </div>
          ))}
          <div>
            <dt>Evaluator</dt>
            <dd>{report.evaluatorDigest}</dd>
          </div>
          <div>
            <dt>Tool implementation</dt>
            <dd>{report.toolImplementationDigest}</dd>
          </div>
          <div>
            <dt>Host source</dt>
            <dd>{report.hostSourceDigest}</dd>
          </div>
          <div>
            <dt>Plan</dt>
            <dd>{report.planDigest}</dd>
          </div>
        </dl>
      </details>
      {report.error && (
        <p role="alert" className="text-danger">
          {report.error}
        </p>
      )}
      {!!report.limitations.length && (
        <ul className="space-y-1 text-xs text-ink-3">
          {Array.from(new Set(report.limitations)).map((item) => (
            <li key={item}>{item}</li>
          ))}
        </ul>
      )}
      <p className="text-xs text-ink-3">
        This evidence does not establish document rendering, spreadsheet
        calculation, or general model quality. Reports do not approve changes.
      </p>
      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          className={ghostBtnCls}
          disabled={busy}
          onClick={onRefresh}
        >
          Refresh report
        </button>
        {report.status === 'running' && (
          <button
            type="button"
            className={ghostBtnCls}
            disabled={busy}
            onClick={onCancel}
          >
            Cancel comparison
          </button>
        )}
      </div>
    </article>
  );
}

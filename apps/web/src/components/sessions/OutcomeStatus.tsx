import { useEffect, useRef, useState } from 'react';
import type { OutcomeCriterion, OutcomeStatus as Status, Run, RunOutcome } from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';
import { useLatest } from '../../hooks/useLatest';

const LABELS: Record<Status, string> = {
  pending: 'Success criteria pending', checking: 'Checking success criteria',
  correcting: 'Correcting the result', criteria_met: 'Success criteria met',
  awaiting_owner: 'Success criteria need your acceptance', accepted: 'Success criteria accepted by you',
  needs_input: 'Success criteria need input', needs_review: 'Success criteria need review',
  exhausted: 'Success criteria reached the run’s limit', stopped: 'Success criteria stopped',
};

export function describeOutcomeCriterion(criterion: OutcomeCriterion): string {
  switch (criterion.kind) {
    case 'response_text': return `Reply contains “${criterion.contains}”`;
    case 'document': return `${criterion.path}: ${criterion.check === 'complete_rendering' ? 'Complete rendering required' : 'Recalculation without errors required'}`;
    case 'file_text': return `${criterion.path} contains “${criterion.contains}”`;
    case 'file_sha256': return `${criterion.path} matches SHA-256 ${criterion.sha256}`;
    case 'file_json': return `${criterion.path}: JSON value at ${criterion.pointer || '(root)'} must equal ${JSON.stringify(criterion.expected)}`;
    case 'artifact_inspection': return `Artifact inspection ${criterion.reportSha256}, ${criterion.locator}`;
    case 'quality': return criterion.rubric;
  }
}

/** Kept beside the run's own reply. Ordinary runs render nothing and make no requests. */
export function OutcomeStatus({ run, enabled = true }: { run: Run; enabled?: boolean }) {
  const runIdentity = `${run.agentId}\u0000${run.id}`;
  const source = run.outcome;
  const [settled, setSettled] = useState<{ runIdentity: string; source: RunOutcome; value: RunOutcome } | null>(null);
  const shown = source && settled?.runIdentity === runIdentity && settled.source === source ? settled.value : source;
  const [busy, setBusy] = useState<{ runIdentity: string; value: RunOutcome } | null>(null);
  const [error, setError] = useState<{ runIdentity: string; source: RunOutcome; text: string } | null>(null);
  const latest = useLatest({ runIdentity, source, shown });
  const sequence = useRef(0);
  useEffect(() => () => { sequence.current += 1; }, []);
  if (!source || !shown) return null;
  const current = shown;
  const inFlight = busy?.runIdentity === runIdentity && busy.value === current;
  const currentError = error?.runIdentity === runIdentity && error.source === current ? error.text : null;
  const request = async (action: 'accept' | 'refresh' | 'close') => {
    if (!enabled || inFlight) return;
    const ticket = ++sequence.current;
    const original = source;
    setBusy({ runIdentity, value: current });
    setError(null);
    const isCurrent = () => ticket === sequence.current && latest.current.runIdentity === runIdentity && latest.current.source === original && latest.current.shown === current;
    try {
      const value = action === 'close' ? await daemon.outcomes.close(run.agentId,run.id,{revision:current.revision}) : action === 'accept' && current.reportSha256
        ? await daemon.outcomes.accept(run.agentId, run.id, { revision: current.revision, reportSha256: current.reportSha256 })
        : await daemon.outcomes.get(run.agentId, run.id);
      if (isCurrent()) setSettled({ runIdentity, source: original, value });
    } catch {
      if (isCurrent()) setError({ runIdentity, source: current, text: action === 'close' ? 'This task could not be closed. Refresh its report before trying again.' : action === 'accept'
        ? 'This report could not be accepted. Refresh the report before trying again.'
        : 'The report could not be refreshed. Try again when connected.' });
    } finally {
      if (ticket === sequence.current) setBusy(null);
    }
  };
  const canAccept = current.status === 'awaiting_owner' && current.definition.acceptance === 'owner_required' && /^[a-f0-9]{64}$/i.test(current.reportSha256 ?? '');
  return <details className="my-2 max-w-3xl rounded-xl border border-line bg-panel px-3 py-2 text-xs text-ink-2">
    <summary className="cursor-pointer" aria-live="polite">{LABELS[current.status]}<span className="ml-2 text-ink-3">{current.consumedCorrections}/{current.definition.maxCorrections ?? 2} corrections</span></summary>
    <div className="mt-2 grid gap-2 break-words">
      <ul className="list-disc pl-4">{current.definition.criteria.map((criterion) => <li key={criterion.id}>{describeOutcomeCriterion(criterion)}</li>)}</ul>
      {current.definition.criteria.some((criterion) => criterion.kind === 'quality') && <p>A quality criterion needs an independent verifier. Text and file identity checks cannot satisfy it.</p>}
      <p>Meeting criteria is separate from your acceptance. File checks use the versions observed by the daemon.</p>
      <details><summary>Evidence identity</summary><dl className="grid gap-1">
        <dt>Run</dt><dd className="font-mono">{current.rootRunId}</dd>
        <dt>Revision</dt><dd>{current.revision}</dd>
        {current.reportSha256 && <><dt>Report SHA-256</dt><dd className="font-mono">{current.reportSha256}</dd></>}
        {current.acceptedReportSha256 && <><dt>Accepted report SHA-256</dt><dd className="font-mono">{current.acceptedReportSha256}</dd></>}
        <dt>Task SHA-256</dt><dd className="font-mono">{current.taskSha256}</dd>
        <dt>Source SHA-256</dt><dd className="font-mono">{current.sourceSha256}</dd>
        <dt>Model SHA-256</dt><dd className="font-mono">{current.modelSha256}</dd>
        <dt>Workspace SHA-256</dt><dd className="font-mono">{current.workspaceSha256}</dd>
        {current.terminalReplySha256 && <><dt>Final reply SHA-256</dt><dd className="font-mono">{current.terminalReplySha256}</dd></>}
      </dl></details>
      {current.iterations.map((iteration) => <details key={iteration.ordinal}>
        <summary>Check {iteration.ordinal}</summary>
        <p className="font-mono">Binding: {iteration.bindingSha256}</p>
        <p className="font-mono">Reply: {iteration.replySha256}</p>
        <ul>{iteration.observations.map((observation) => <li key={observation.criterionId} className="mt-2">
          <p>{observation.criterionId}: {observation.verdict === 'met' ? 'Met' : observation.verdict === 'not_met' ? 'Not met' : 'Inconclusive'}</p>
          <p>{observation.detail}</p>
          {observation.subjectSha256 && <p className="font-mono">Subject: {observation.subjectSha256}</p>}
          {observation.reportSha256 && <p className="font-mono">Inspection: {observation.reportSha256}</p>}
          {observation.locator && <p>Evidence: {observation.locator}</p>}
        </li>)}</ul>
      </details>)}
      {currentError && <p role="alert" className="text-danger">{currentError}</p>}
      <div className="flex gap-3">
        {canAccept && <button type="button" className="underline" disabled={!enabled || inFlight || Boolean(currentError)} onClick={() => void request('accept')}>{inFlight ? 'Saving…' : 'Accept this report'}</button>}
        <button type="button" className="underline" disabled={!enabled || inFlight} onClick={() => void request('refresh')}>Refresh report</button>
        {['needs_input','needs_review','exhausted','awaiting_owner'].includes(current.status) && <button type="button" className="underline" disabled={!enabled || inFlight} onClick={() => void request('close')}>Close task</button>}
      </div>
    </div>
  </details>;
}

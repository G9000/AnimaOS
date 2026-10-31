import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import type { RunOutcome } from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';
import { runFixture } from '../../test/live';
import { OutcomeStatus } from './OutcomeStatus';

const outcome: RunOutcome = {
  definition: { criteria: [{ id: 'result', kind: 'response_text', contains: 'Ready' }], acceptance: 'owner_required' },
  revision: 3, rootRunId: 'root', taskSha256: 'a'.repeat(64), sourceSha256: 'b'.repeat(64),
  modelSha256: 'c'.repeat(64), workspaceSha256: 'd'.repeat(64), consumedCorrections: 1,
  status: 'awaiting_owner', reportSha256: 'e'.repeat(64), repairPaths: [],
  iterations: [{ ordinal: 1, bindingSha256: 'f'.repeat(64), semanticSha256:'3'.repeat(64), replySha256: '1'.repeat(64), observations: [{ criterionId: 'result', verdict: 'met', detail: 'Required text found.', subjectSha256: '2'.repeat(64) }] }],
};
afterEach(() => vi.restoreAllMocks());

it('renders nothing for ordinary runs and keeps criteria met distinct from acceptance', () => {
  const { rerender } = render(<OutcomeStatus run={runFixture('ordinary')} />);
  expect(screen.queryByText(/Success criteria/)).toBeNull();
  rerender(<OutcomeStatus run={runFixture('checked', { outcome: { ...outcome, status: 'criteria_met', definition: { ...outcome.definition, acceptance: 'report_only' } } })} />);
  expect(screen.getByText('Success criteria met')).toBeInTheDocument();
  expect(screen.queryByRole('button', { name: 'Accept this report' })).toBeNull();
});

it('shows the exact evidence and accepts only the displayed revision and digest', async () => {
  const accept = vi.spyOn(daemon.outcomes, 'accept').mockResolvedValue({ ...outcome, status: 'accepted', acceptedReportSha256: outcome.reportSha256 });
  render(<OutcomeStatus run={runFixture('checked', { outcome })} />);
  fireEvent.click(screen.getByText('Success criteria need your acceptance'));
  expect(screen.getByText(outcome.reportSha256!)).toBeInTheDocument();
  expect(screen.getByText('Required text found.')).toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'Accept this report' }));
  await waitFor(() => expect(screen.getByText('Success criteria accepted by you')).toBeInTheDocument());
  expect(accept).toHaveBeenCalledWith('agent-main', 'checked', { revision: 3, reportSha256: outcome.reportSha256 });
});

it('does not let a late acceptance replace a newer report and preserves Stop as an outcome', async () => {
  let settle!: (value: RunOutcome) => void;
  vi.spyOn(daemon.outcomes, 'accept').mockReturnValue(new Promise((resolve) => { settle = resolve; }));
  const { rerender } = render(<OutcomeStatus run={runFixture('checked', { outcome })} />);
  fireEvent.click(screen.getByText('Success criteria need your acceptance'));
  fireEvent.click(screen.getByRole('button', { name: 'Accept this report' }));
  rerender(<OutcomeStatus run={runFixture('checked', { outcome: { ...outcome, revision: 4, status: 'stopped' } })} />);
  await act(async () => settle({ ...outcome, status: 'accepted' }));
  expect(screen.getByText('Success criteria stopped')).toBeInTheDocument();
  expect(screen.queryByText('Success criteria accepted by you')).toBeNull();
});

it('does not show a previous run’s accepted result when another run shares the source object', async () => {
  vi.spyOn(daemon.outcomes, 'accept').mockResolvedValue({ ...outcome, status: 'accepted' });
  const { rerender } = render(<OutcomeStatus run={runFixture('first', { outcome })} />);
  fireEvent.click(screen.getByText('Success criteria need your acceptance'));
  fireEvent.click(screen.getByRole('button', { name: 'Accept this report' }));
  await screen.findByText('Success criteria accepted by you');
  rerender(<OutcomeStatus run={runFixture('second', { outcome })} />);
  expect(screen.getByText('Success criteria need your acceptance')).toBeInTheDocument();
  expect(screen.queryByText('Success criteria accepted by you')).toBeNull();
  expect(screen.getByRole('button', { name: 'Accept this report' })).toBeEnabled();
});

it('keeps a replacement agent’s report usable and ignores a late failure for the previous agent', async () => {
  let reject!: (error: Error) => void;
  vi.spyOn(daemon.outcomes, 'accept').mockReturnValue(new Promise((_resolve, rejectPromise) => { reject = rejectPromise; }));
  const { rerender } = render(<OutcomeStatus run={runFixture('checked', { outcome })} />);
  fireEvent.click(screen.getByText('Success criteria need your acceptance'));
  fireEvent.click(screen.getByRole('button', { name: 'Accept this report' }));
  rerender(<OutcomeStatus run={runFixture('checked', { agentId: 'agent-other', outcome })} />);
  expect(screen.getByRole('button', { name: 'Accept this report' })).toBeEnabled();
  await act(async () => reject(new Error('Old agent report changed')));
  expect(screen.queryByRole('alert')).toBeNull();
  expect(screen.getByText('Success criteria need your acceptance')).toBeInTheDocument();
});

it('keeps stale acceptance errors visible and allows an explicit report refresh', async () => {
  vi.spyOn(daemon.outcomes, 'accept').mockRejectedValue(new Error('Report changed'));
  const get = vi.spyOn(daemon.outcomes, 'get').mockResolvedValue({ ...outcome, revision: 4, status: 'needs_review', reportSha256: null });
  render(<OutcomeStatus run={runFixture('checked', { outcome })} />);
  fireEvent.click(screen.getByText('Success criteria need your acceptance'));
  fireEvent.click(screen.getByRole('button', { name: 'Accept this report' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Refresh the report before trying again.');
  fireEvent.click(screen.getByRole('button', { name: 'Refresh report' }));
  await waitFor(() => expect(screen.getByText('Success criteria need review')).toBeInTheDocument());
  expect(get).toHaveBeenCalledWith('agent-main', 'checked');
});

it('keeps unavailable quality evidence inconclusive and disables owner actions offline', () => {
  const get = vi.spyOn(daemon.outcomes, 'get');
  const accept = vi.spyOn(daemon.outcomes, 'accept');
  const quality: RunOutcome = {
    ...outcome, status: 'needs_review', reportSha256: null,
    definition: { criteria: [{ id: 'quality', kind: 'quality', rubric: 'The summary is clear and complete.' }], acceptance: 'owner_required' },
    iterations: [{ ordinal: 1, bindingSha256: 'f'.repeat(64), semanticSha256:'3'.repeat(64), replySha256: '1'.repeat(64), observations: [{ criterionId: 'quality', verdict: 'inconclusive', detail: 'No independent verifier configured.' }] }],
  };
  render(<OutcomeStatus run={runFixture('quality', { outcome: quality })} enabled={false} />);
  fireEvent.click(screen.getByText('Success criteria need review'));
  expect(screen.getByText('A quality criterion needs an independent verifier. Text and file identity checks cannot satisfy it.')).toBeInTheDocument();
  expect(screen.getByText('No independent verifier configured.')).toBeInTheDocument();
  expect(screen.queryByRole('button', { name: 'Accept this report' })).toBeNull();
  expect(screen.getByRole('button', { name: 'Refresh report' })).toBeDisabled();
  expect(get).not.toHaveBeenCalled();
  expect(accept).not.toHaveBeenCalled();
});

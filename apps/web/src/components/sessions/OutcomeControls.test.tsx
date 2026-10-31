import { fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import { expect, it } from 'vitest';
import { EMPTY_OUTCOME_DRAFT, OutcomeControls, outcomeDefinitionFromDraft } from './OutcomeControls';

it('keeps criteria omitted until explicitly enabled and preserves literal required text', () => {
  expect(outcomeDefinitionFromDraft(EMPTY_OUTCOME_DRAFT)).toEqual({});
  expect(outcomeDefinitionFromDraft({ ...EMPTY_OUTCOME_DRAFT, enabled: true, contains: ' Ready ' })).toEqual({ definition: {
    criteria: [{ id: 'result', kind: 'response_text', contains: ' Ready ' }],
    maxCorrections: 2, acceptance: 'report_only',
  } });
});

it('requires a file path for a file check and carries the chosen correction cap', () => {
  const draft = { ...EMPTY_OUTCOME_DRAFT, enabled: true, kind: 'file_text' as const, contains: 'Ready', maxCorrections: 0, ownerRequired: true };
  expect(outcomeDefinitionFromDraft(draft)).toEqual({ error: 'Enter the workspace file to check.' });
  expect(outcomeDefinitionFromDraft({ ...draft, path: ' reports/summary.txt ' })).toEqual({ definition: {
    criteria: [{ id: 'result', kind: 'file_text', path: 'reports/summary.txt', contains: 'Ready' }],
    maxCorrections: 0, acceptance: 'owner_required',
  } });
  expect(outcomeDefinitionFromDraft({ ...draft, path: 'report.txt', maxCorrections: 3 }).error).toBe('Choose between 0 and 2 automatic corrections.');
});

it.each(['complete_rendering', 'recalculated_without_errors'] as const)('converts %s to an exact document check without required text', (kind) => {
  const draft = { ...EMPTY_OUTCOME_DRAFT, enabled: true, kind, maxCorrections: 0, ownerRequired: true };
  expect(outcomeDefinitionFromDraft(draft)).toEqual({ error: 'Enter the workspace file to check.' });
  expect(outcomeDefinitionFromDraft({ ...draft, path: ' reports/result.xlsx ' })).toEqual({ definition: {
    criteria: [{ id: 'result', kind: 'document', path: 'reports/result.xlsx', check: kind }],
    maxCorrections: 0, acceptance: 'owner_required',
  } });
});

it.each([
  ['complete_rendering', 'Document renders fully', 'Checks whether every page in the exported document view was rendered. This does not assess layout or visual quality.'],
  ['recalculated_without_errors', 'Spreadsheet recalculates without errors', 'Checks all formulas in the original XLSX spreadsheet for recalculation errors. A spreadsheet without formulas also meets this check.'],
] as const)('shows only the path and selected explanation for %s', (kind, label, explanation) => {
  function Controls() {
    const [draft, setDraft] = useState({ ...EMPTY_OUTCOME_DRAFT });
    return <OutcomeControls value={draft} onChange={setDraft} />;
  }
  render(<Controls />);
  fireEvent.click(screen.getByText('Success criteria'));
  fireEvent.click(screen.getByRole('checkbox', { name: 'Check this result' }));
  fireEvent.change(screen.getByRole('combobox', { name: 'Check' }), { target: { value: kind } });
  expect(screen.getByRole('option', { name: label })).toHaveProperty('selected', true);
  expect(screen.getByRole('textbox', { name: 'Workspace file' })).toBeInTheDocument();
  expect(screen.queryByRole('textbox', { name: 'Required text' })).not.toBeInTheDocument();
  expect(screen.getByText(explanation)).toBeInTheDocument();
  expect(screen.getByText('If the check is unavailable or incomplete, the result stays unverified.')).toBeInTheDocument();
  expect(screen.queryByText(/configured independent verifier/)).not.toBeInTheDocument();
  fireEvent.change(screen.getByRole('combobox', { name: 'Check' }), { target: { value: 'response_text' } });
  expect(screen.getByRole('textbox', { name: 'Required text' })).toBeInTheDocument();
  expect(screen.queryByRole('textbox', { name: 'Workspace file' })).not.toBeInTheDocument();
});

it('offers bounded fields rather than raw definitions, and disabling the controls prevents edits', () => {
  function Controls() {
    const [draft, setDraft] = useState({ ...EMPTY_OUTCOME_DRAFT });
    return <OutcomeControls value={draft} onChange={setDraft} />;
  }
  const { rerender } = render(<Controls />);
  fireEvent.click(screen.getByText('Success criteria'));
  fireEvent.click(screen.getByRole('checkbox', { name: 'Check this result' }));
  fireEvent.change(screen.getByRole('combobox', { name: 'Check' }), { target: { value: 'file_text' } });
  expect(screen.getByRole('textbox', { name: 'Workspace file' })).toBeInTheDocument();
  expect(Array.from((screen.getByRole('combobox', { name: 'Maximum automatic corrections' }) as HTMLSelectElement).options).map((option) => option.value)).toEqual(['0', '1', '2']);
  rerender(<OutcomeControls value={{ ...EMPTY_OUTCOME_DRAFT, enabled: true }} onChange={() => {}} disabled />);
  fireEvent.click(screen.getByText('Success criteria · enabled for next message'));
  expect(screen.getByRole('textbox', { name: 'Required text' })).toBeDisabled();
});

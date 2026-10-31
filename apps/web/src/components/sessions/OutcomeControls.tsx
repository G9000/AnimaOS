import type { OutcomeCriterion, OutcomeDefinition } from '@animaos/sdk';

export interface OutcomeDraft {
  enabled: boolean;
  kind: 'response_text' | 'file_text' | 'complete_rendering' | 'recalculated_without_errors';
  path: string;
  contains: string;
  maxCorrections: number;
  ownerRequired: boolean;
}

export const EMPTY_OUTCOME_DRAFT: OutcomeDraft = {
  enabled: false, kind: 'response_text', path: '', contains: '',
  maxCorrections: 2, ownerRequired: false,
};

export function outcomeDefinitionFromDraft(draft: OutcomeDraft): { definition?: OutcomeDefinition; error?: string } {
  if (!draft.enabled) return {};
  if ((draft.kind === 'response_text' || draft.kind === 'file_text') && !draft.contains.trim()) return { error: 'Enter the text the result must contain.' };
  if (draft.kind !== 'response_text' && !draft.path.trim()) return { error: 'Enter the workspace file to check.' };
  if (!Number.isInteger(draft.maxCorrections) || draft.maxCorrections < 0 || draft.maxCorrections > 2) return { error: 'Choose between 0 and 2 automatic corrections.' };
  const criterion: OutcomeCriterion = draft.kind === 'response_text'
    ? { id: 'result', kind: 'response_text', contains: draft.contains }
    : draft.kind === 'file_text'
      ? { id: 'result', kind: 'file_text', path: draft.path.trim(), contains: draft.contains }
      : { id: 'result', kind: 'document', path: draft.path.trim(), check: draft.kind };
  return { definition: {
    criteria: [criterion],
    maxCorrections: draft.maxCorrections,
    acceptance: draft.ownerRequired ? 'owner_required' : 'report_only',
  } };
}

const CHECK_DESCRIPTIONS: Record<OutcomeDraft['kind'], string> = {
  response_text: 'Checks whether the reply contains the required text.',
  file_text: 'Checks whether the workspace file contains the required text, using existing file access.',
  complete_rendering: 'Checks whether every page in the exported document view was rendered. This does not assess layout or visual quality.',
  recalculated_without_errors: 'Checks all formulas in the original XLSX spreadsheet for recalculation errors. A spreadsheet without formulas also meets this check.',
};

/** Per-message opt-in for exact text or finite document checks. */
export function OutcomeControls({ value, onChange, disabled = false }: {
  value: OutcomeDraft;
  onChange(value: OutcomeDraft): void;
  disabled?: boolean;
}) {
  const documentCheck = value.kind === 'complete_rendering' || value.kind === 'recalculated_without_errors';
  return <details className="mb-2 rounded-xl border border-line bg-panel px-3 py-2 text-xs text-ink-2">
    <summary className="cursor-pointer">Success criteria{value.enabled ? ' · enabled for next message' : ''}</summary>
    <fieldset disabled={disabled} className="mt-2 grid gap-2">
      <label className="flex items-center gap-2"><input type="checkbox" checked={value.enabled} onChange={(event) => onChange({ ...value, enabled: event.target.checked })} />Check this result</label>
      {value.enabled && <>
        <label className="grid gap-1">Check
          <select className="rounded border border-line bg-panel p-1" value={value.kind} onChange={(event) => onChange({ ...value, kind: event.target.value as OutcomeDraft['kind'] })}>
            <option value="response_text">Reply contains text</option>
            <option value="file_text">Workspace file contains text</option>
            <option value="complete_rendering">Document renders fully</option>
            <option value="recalculated_without_errors">Spreadsheet recalculates without errors</option>
          </select>
        </label>
        {value.kind !== 'response_text' && <label className="grid gap-1">Workspace file<input className="rounded border border-line bg-panel p-1" value={value.path} maxLength={512} placeholder={value.kind === 'recalculated_without_errors' ? 'reports/results.xlsx' : value.kind === 'complete_rendering' ? 'reports/summary.docx' : 'reports/summary.txt'} onChange={(event) => onChange({ ...value, path: event.target.value })} /></label>}
        {!documentCheck && <label className="grid gap-1">Required text<input className="rounded border border-line bg-panel p-1" value={value.contains} maxLength={8192} onChange={(event) => onChange({ ...value, contains: event.target.value })} /></label>}
        <label className="grid gap-1">Maximum automatic corrections
          <select className="rounded border border-line bg-panel p-1" value={value.maxCorrections} onChange={(event) => onChange({ ...value, maxCorrections: Number(event.target.value) })}>
            <option value={0}>0</option><option value={1}>1</option><option value={2}>2</option>
          </select>
        </label>
        <label className="flex items-center gap-2"><input type="checkbox" checked={value.ownerRequired} onChange={(event) => onChange({ ...value, ownerRequired: event.target.checked })} />Require my acceptance</label>
        <p>{CHECK_DESCRIPTIONS[value.kind]}</p>
        {documentCheck && <p>If the check is unavailable or incomplete, the result stays unverified.</p>}
        <p>Automatic corrections stay within the run’s limits.</p>
      </>}
    </fieldset>
  </details>;
}

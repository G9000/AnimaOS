import { useState } from 'react';

import { COMPANION_UNREACHABLE } from '../../lib/approvals';
import { useAppearancePreferences } from '../../lib/appearance-preferences';
import {
  applyLook,
  changeLook,
  describeLookChange,
  sameLook,
  type LookChange,
} from '../../lib/owner-look';

/** A look the companion changed from chat, with Undo while it is still the
 *  look in effect. */
export function LookChangeNotice({ change }: { change: LookChange }) {
  const [appearance] = useAppearancePreferences();
  const [undoing, setUndoing] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const inEffect = sameLook(appearance, change.look);
  const undo = async () => {
    setUndoing(true);
    setProblem(null);
    try {
      await changeLook(change.previous);
    } catch {
      // The daemon still holds the change: show it again here.
      applyLook(change.look);
      setProblem(COMPANION_UNREACHABLE);
    }
    setUndoing(false);
  };
  const summary = describeLookChange(change);
  return (
    <div
      className="automation-notice look-notice"
      role="note"
      aria-label={`Look changed: ${summary}`}
    >
      <p className="automation-notice-title">
        <span className="automation-badge">Look changed</span> {summary}
      </p>
      {inEffect && (
        <button
          type="button"
          className="studio-tool-button"
          disabled={undoing}
          onClick={() => void undo()}
        >
          {undoing ? 'Undoing…' : 'Undo'}
        </button>
      )}
      {problem && (
        <p className="automation-problem" role="status">
          {problem}
        </p>
      )}
    </div>
  );
}

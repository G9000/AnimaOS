import { useEffect, useRef, useState, type FormEvent } from 'react';

import {
  daemon,
  MAX_DENY_PATTERN_CHARS,
  MAX_DENY_PATTERNS,
  type AgentAutonomy,
  type AutonomyPatch,
} from '../../lib/daemon-api';
import { Switch } from '../ui-bits';

type PresetMode = 'free' | 'strict';

const MODES: readonly { value: PresetMode; label: string; summary: string }[] =
  [
    {
      value: 'free',
      label: 'Free',
      summary:
        'Works on its own and tells you what it did. Dangerous things still ask.',
    },
    {
      value: 'strict',
      label: 'Strict',
      summary: 'Asks before it changes or runs anything.',
    },
  ];

/** What asks in every mode (spec 2026-10-19, "the floor"). */
export const ALWAYS_ASKS: readonly string[] = [
  'Sending email',
  'Calendar changes',
  'Editing AGENTS.md or CLAUDE.md',
  'Installing bundles or MCP servers',
  'Handing keys to apps',
];

function errorText(caught: unknown): string {
  if (
    caught &&
    typeof caught === 'object' &&
    (caught as { status?: number }).status === 409
  )
    return "Helpers follow their companion's autonomy.";
  return caught instanceof Error ? caught.message : String(caught);
}

/** Why a new blocked command can't be added, or null when it can. */
export function denyPatternProblem(
  pattern: string,
  existing: readonly string[],
): string | null {
  const value = pattern.trim();
  if (!value) return 'Type a command to block.';
  if (value.length > MAX_DENY_PATTERN_CHARS)
    return `Keep it to ${MAX_DENY_PATTERN_CHARS} characters.`;
  if (/[\u0000-\u001f\u007f]/.test(value)) return 'Keep it to one line.';
  if (existing.includes(value)) return 'That command is already blocked.';
  if (existing.length >= MAX_DENY_PATTERNS)
    return `You can block up to ${MAX_DENY_PATTERNS} commands.`;
  return null;
}

/**
 * How much the agent asks (spec 2026-10-19): Free or Strict, Custom when
 * the class rules below match neither, the optional smart check, the
 * owner's own blocked commands, and what asks in every mode.
 */
export function AutonomyCard({
  agentId,
  revision = 0,
  onModeChanged,
}: {
  agentId: string;
  /** Bumped when the class rules changed elsewhere: read it again. */
  revision?: number;
  /** A preset changed the class rules: whoever shows them reads them again. */
  onModeChanged?: () => void;
}) {
  const [autonomy, setAutonomy] = useState<AgentAutonomy | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [draft, setDraft] = useState('');
  const [problem, setProblem] = useState<string | null>(null);
  const generation = useRef(0);

  useEffect(() => {
    const current = ++generation.current;
    setError(null);
    daemon.autonomy(agentId).then(
      (result) => {
        if (generation.current === current) setAutonomy(result);
      },
      (caught: unknown) => {
        if (generation.current === current) setError(errorText(caught));
      },
    );
  }, [agentId, revision]);

  useEffect(() => {
    setAutonomy(null);
    setDraft('');
    setProblem(null);
  }, [agentId]);

  const save = async (patch: AutonomyPatch): Promise<boolean> => {
    if (!autonomy || saving) return false;
    const current = generation.current;
    setSaving(true);
    setError(null);
    try {
      const result = await daemon.setAutonomy(agentId, patch);
      if (generation.current !== current) return false;
      setAutonomy(result);
      if (patch.mode) onModeChanged?.();
      return true;
    } catch (caught) {
      if (generation.current === current) setError(errorText(caught));
      return false;
    } finally {
      if (generation.current === current) setSaving(false);
    }
  };

  const addPattern = async (event: FormEvent) => {
    event.preventDefault();
    if (!autonomy) return;
    const reason = denyPatternProblem(draft, autonomy.denyPatterns);
    setProblem(reason);
    if (reason) return;
    if (
      await save({ denyPatterns: [...autonomy.denyPatterns, draft.trim()] })
    )
      setDraft('');
  };

  const removePattern = (pattern: string) => {
    if (!autonomy) return;
    setProblem(null);
    void save({
      denyPatterns: autonomy.denyPatterns.filter((item) => item !== pattern),
    });
  };

  return (
    <section
      className="approvals-section autonomy"
      aria-labelledby="approvals-autonomy"
    >
      <h2 id="approvals-autonomy">Autonomy</h2>
      <p className="approvals-help">
        How much anima asks before it acts. Everything it does shows up in
        Work, and you can stop it any time.
      </p>
      {error && (
        <p className="approvals-error" role="alert">
          {error}
        </p>
      )}
      {autonomy === null ? (
        !error && <p className="approvals-empty">Reading autonomy…</p>
      ) : (
        <>
          <div
            className="autonomy-modes"
            role="radiogroup"
            aria-label="How much it asks"
          >
            {MODES.map((mode) => (
              <label
                key={mode.value}
                className="autonomy-mode"
                data-selected={autonomy.mode === mode.value || undefined}
              >
                <input
                  type="radio"
                  name={`autonomy-${agentId}`}
                  value={mode.value}
                  checked={autonomy.mode === mode.value}
                  disabled={saving}
                  onChange={() => void save({ mode: mode.value })}
                />
                <span className="autonomy-mode-title">
                  {mode.label}
                  {mode.value === 'free' && (
                    <span className="autonomy-tag">Recommended</span>
                  )}
                </span>
                <span className="autonomy-mode-summary">{mode.summary}</span>
              </label>
            ))}
            <div
              className="autonomy-mode autonomy-mode-custom"
              data-selected={autonomy.mode === 'custom' || undefined}
              aria-current={autonomy.mode === 'custom' || undefined}
            >
              <span className="autonomy-mode-title">Custom</span>
              <span className="autonomy-mode-summary">
                {autonomy.mode === 'custom'
                  ? "You've set your own rules below."
                  : 'Shows up when you set your own rules below.'}
              </span>
            </div>
          </div>

          <div className="autonomy-smart">
            <div>
              <p className="autonomy-smart-title">Smart check</p>
              <p className="approvals-help">
                Let anima's model approve risky commands it judges safe (extra
                model call each time).
              </p>
            </div>
            <Switch
              checked={autonomy.smartCheck}
              disabled={saving}
              label="Smart check"
              onChange={(on) => void save({ smartCheck: on })}
            />
          </div>

          <div className="autonomy-floor">
            <div className="autonomy-floor-column">
              <h3 id="autonomy-blocked">Always blocked</h3>
              <p className="approvals-help">
                Commands matching these never run, in any mode. Use * and ? as
                wildcards.
              </p>
              {autonomy.denyPatterns.length > 0 && (
                <ul
                  className="autonomy-list"
                  aria-labelledby="autonomy-blocked"
                >
                  {autonomy.denyPatterns.map((pattern) => (
                    <li key={pattern}>
                      <code>{pattern}</code>
                      <button
                        type="button"
                        className="studio-tool-button"
                        aria-label={`Unblock ${pattern}`}
                        disabled={saving}
                        onClick={() => removePattern(pattern)}
                      >
                        Remove
                      </button>
                    </li>
                  ))}
                </ul>
              )}
              <form
                className="autonomy-add"
                aria-label="Block a command"
                onSubmit={(event) => void addPattern(event)}
              >
                <input
                  aria-label="Command to block"
                  value={draft}
                  placeholder="e.g. npm publish*"
                  maxLength={MAX_DENY_PATTERN_CHARS + 1}
                  aria-invalid={problem ? true : undefined}
                  onChange={(event) => {
                    setDraft(event.target.value);
                    setProblem(null);
                  }}
                />
                <button
                  type="submit"
                  className="studio-tool-button"
                  disabled={saving}
                >
                  Add
                </button>
              </form>
              {problem && (
                <p className="approvals-error" role="alert">
                  {problem}
                </p>
              )}
              <p className="approvals-help">
                Up to {MAX_DENY_PATTERNS}. Deleting system folders, formatting
                disks and similar are always blocked too.
              </p>
            </div>
            <div className="autonomy-floor-column">
              <h3 id="autonomy-always-asks">Always asks, in any mode</h3>
              <p className="approvals-help">
                Free never skips these. You can relax one at a time in the
                rules below.
              </p>
              <ul
                className="autonomy-list"
                aria-labelledby="autonomy-always-asks"
              >
                {ALWAYS_ASKS.map((item) => (
                  <li key={item}>
                    <span>{item}</span>
                    <span className="autonomy-asks">Asks</span>
                  </li>
                ))}
              </ul>
            </div>
          </div>
        </>
      )}
    </section>
  );
}

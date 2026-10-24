import { useId, useState } from 'react';
import type { AgentDetail } from '../lib/types';
import {
  assignedOrbColors,
  DEFAULT_AGENT_ORB_STYLE,
  orbEnergy,
  orbPixel,
  ORB_COLORS,
  saveAgentOrbStyle,
  useAgentOrbStyle,
  type AgentOrbStyle,
} from '../lib/orb-style-preferences';
import { HaloOrb } from './halo/HaloOrb';

const MOTIONS = [
  ['calm', 'Calm', 'SLOW DRIFT'],
  ['lively', 'Lively', 'DEFAULT'],
  ['restless', 'Restless', 'ALWAYS BUSY'],
] as const;
const GRAINS = [
  ['smooth', 'Smooth', 'PIXEL 1'],
  ['fine', 'Fine', 'PIXEL 1.5'],
  ['chunky', 'Chunky', 'PIXEL 4'],
] as const;

export function OrbStyleDialog({
  agent,
  agents,
  onClose,
}: {
  agent: AgentDetail;
  agents: readonly AgentDetail[];
  onClose: () => void;
}) {
  const titleId = useId();
  const saved = useAgentOrbStyle(agent.id);
  const [draft, setDraft] = useState<AgentOrbStyle>(saved);
  const assignments = assignedOrbColors();
  const set = <K extends keyof AgentOrbStyle>(key: K, value: AgentOrbStyle[K]) =>
    setDraft((current) => ({ ...current, [key]: value }));

  return (
    <div className="orb-style-backdrop" onMouseDown={(event) => {
      if (event.target === event.currentTarget) onClose();
    }}>
      <section
        className="orb-style-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
      >
        <header className="orb-style-header">
          <div>
            <p>ORB STYLE · {agent.name.toUpperCase()}</p>
            <h2 id={titleId}>Make it yours</h2>
          </div>
          <button type="button" aria-label="Close orb style" onClick={onClose}>
            ×
          </button>
        </header>

        <div className="orb-style-preview">
          <HaloOrb
            accent={draft.color}
            energy={orbEnergy(draft.motion)}
            pixel={orbPixel(draft.grain)}
            className="orb-style-preview-orb"
          />
          <span>LIVE PREVIEW · LISTENING</span>
        </div>

        <fieldset className="orb-style-fieldset">
          <legend>COLOUR</legend>
          <div className="orb-style-colors">
            {ORB_COLORS.map((color) => {
              const users = agents.filter(
                (item) =>
                  item.id !== agent.id && assignments[item.id] === color.hex,
              );
              const selected = draft.color === color.hex;
              return (
                <button
                  key={color.id}
                  type="button"
                  className={`orb-style-color${selected ? ' is-selected' : ''}`}
                  disabled={users.length > 0 && !selected}
                  aria-label={`${color.name}${selected ? ', current' : ''}${users.length ? `, used by ${users.map((item) => item.name).join(', ')}` : ', free'}`}
                  aria-pressed={selected}
                  title={users.length ? `Used by ${users.map((item) => item.name).join(', ')}` : color.name}
                  onClick={() => set('color', color.hex)}
                >
                  <i style={{ backgroundColor: color.hex }} />
                  <span>{selected ? 'CURRENT' : users.length ? `USED BY ${users.map((item) => item.name.toUpperCase()).join(', ')}` : 'FREE'}</span>
                </button>
              );
            })}
          </div>
        </fieldset>

        <ChoiceGroup
          title="MOTION"
          values={MOTIONS}
          value={draft.motion}
          onChange={(value) => set('motion', value)}
        />
        <ChoiceGroup
          title="GRAIN"
          values={GRAINS}
          value={draft.grain}
          onChange={(value) => set('grain', value)}
        />

        <footer className="orb-style-actions">
          <button
            type="button"
            className="orb-style-reset"
            onClick={() => setDraft(DEFAULT_AGENT_ORB_STYLE)}
          >
            Reset
          </button>
          <span />
          <button type="button" onClick={onClose}>Cancel</button>
          <button
            type="button"
            className="orb-style-save"
            onClick={() => {
              saveAgentOrbStyle(agent.id, draft);
              onClose();
            }}
          >
            Save look
          </button>
        </footer>
      </section>
    </div>
  );
}

function ChoiceGroup<T extends string>({
  title,
  values,
  value,
  onChange,
}: {
  title: string;
  values: readonly (readonly [T, string, string])[];
  value: T;
  onChange: (value: T) => void;
}) {
  return (
    <fieldset className="orb-style-fieldset">
      <legend>{title}</legend>
      <div className="orb-style-choices">
        {values.map(([id, label, note]) => (
          <button
            key={id}
            type="button"
            className={value === id ? 'is-selected' : ''}
            aria-pressed={value === id}
            onClick={() => onChange(id)}
          >
            <strong>{label}</strong>
            <small>{note}</small>
          </button>
        ))}
      </div>
    </fieldset>
  );
}

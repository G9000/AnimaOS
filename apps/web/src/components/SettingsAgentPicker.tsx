import { useEffect, useId, useRef, useState } from 'react';
import { companionAgents } from '../lib/active-agent';
import type { AgentDetail } from '../lib/types';
import { AgentAvatar } from './AgentAvatar';
import { PenIcon } from './PenIcon';
import { HaloOrb } from './halo/HaloOrb';

export function SettingsAgentPicker({
  agent,
  agents,
  onSelect,
  onCreateAgent,
  locked,
}: {
  agent: AgentDetail;
  agents: readonly AgentDetail[];
  onSelect?: (agentId: string) => void;
  onCreateAgent?: () => void;
  locked: boolean;
}) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const menuId = useId();
  const companions = companionAgents(agents);

  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      setOpen(false);
      trigger.current?.focus();
    };
    document.addEventListener('pointerdown', outside);
    document.addEventListener('keydown', escape);
    return () => {
      document.removeEventListener('pointerdown', outside);
      document.removeEventListener('keydown', escape);
    };
  }, [open]);

  const choose = (id: string) => {
    setOpen(false);
    trigger.current?.focus();
    if (id !== agent.id) onSelect?.(id);
  };

  return (
    <div ref={root} className="settings-agent-picker">
      <button
        ref={trigger}
        type="button"
        className="settings-agent-picker-trigger"
        aria-label={`Settings agent: ${agent.name}`}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        disabled={locked}
        onClick={() => setOpen((value) => !value)}
      >
        <AgentAvatar
          id={agent.id}
          name={agent.name}
          size={20}
          fallback={<HaloOrb energy={0.4} className="agent-switch-orb" />}
        />
        <span className="settings-agent-picker-name">{agent.name}</span>
        <PenIcon name="caret-down" size={12} />
      </button>
      {open && (
        <div
          id={menuId}
          className="settings-agent-picker-menu"
          role="menu"
          aria-label="Choose an agent for settings"
        >
          {companions.map((item) => (
            <button
              key={item.id}
              type="button"
              role="menuitemradio"
              aria-checked={item.id === agent.id}
              className="settings-agent-picker-option"
              disabled={locked}
              onClick={() => choose(item.id)}
            >
              <AgentAvatar
                id={item.id}
                name={item.name}
                size={20}
                fallback={<HaloOrb energy={0.4} className="agent-switch-orb" />}
              />
              <span>{item.name}</span>
              {item.id === agent.id && (
                <span className="settings-agent-picker-check" aria-hidden>
                  ✓
                </span>
              )}
            </button>
          ))}
          <div className="settings-agent-picker-divider" role="separator" />
          <button
            type="button"
            role="menuitem"
            className="settings-agent-picker-create"
            disabled={locked || !onCreateAgent}
            onClick={() => {
              setOpen(false);
              onCreateAgent?.();
            }}
          >
            <span aria-hidden="true">+</span>
            New agent…
          </button>
        </div>
      )}
    </div>
  );
}

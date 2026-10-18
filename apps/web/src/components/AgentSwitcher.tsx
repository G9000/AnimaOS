import { useEffect, useId, useRef, useState } from 'react';
import type { AgentDetail } from '../lib/types';
import { companionAgents } from '../lib/active-agent';
import { AgentAvatar } from './AgentAvatar';
import { ChevronIcon, GearIcon } from './icons';

/** The header names who you are talking with: avatar, name, and what it is
 *  doing. With more than one companion it opens a list to switch. */
export function AgentSwitcher({
  agent,
  agents,
  online,
  status,
  busy,
  onSelect,
  onOpenSettings,
}: {
  agent: AgentDetail;
  agents: readonly AgentDetail[];
  online: boolean;
  /** What the agent is doing now, from its presence. */
  status: string;
  /** The agent is doing something, so its dot pulses. */
  busy: boolean;
  onSelect?: (agentId: string) => void;
  onOpenSettings: () => void;
}) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const menuId = useId();
  const companions = companionAgents(agents);

  useEffect(() => {
    if (!open) return;
    const close = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      setOpen(false);
      trigger.current?.focus();
    };
    document.addEventListener('pointerdown', close);
    document.addEventListener('keydown', escape);
    return () => {
      document.removeEventListener('pointerdown', close);
      document.removeEventListener('keydown', escape);
    };
  }, [open]);

  const choose = (id: string) => {
    setOpen(false);
    trigger.current?.focus();
    if (id !== agent.id) onSelect?.(id);
  };

  return (
    <div ref={root} className="agent-switch">
      <h1 className="sr-only">{agent.name}</h1>
      <button
        ref={trigger}
        type="button"
        className="agent-switch-trigger"
        aria-label={`${agent.name}, switch agent`}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        onClick={() => setOpen((value) => !value)}
      >
        <AgentAvatar id={agent.id} name={agent.name} size={30} />
        <span className="agent-switch-text">
          <span className="agent-switch-name">
            <span className="scenic-wordmark" title={agent.name}>
              {agent.name}
            </span>
            <ChevronIcon size={13} />
          </span>
          <span
            className={`companion-status ${online ? 'is-online' : 'is-offline'}${busy ? ' is-busy' : ''}`}
            role="status"
          >
            <i aria-hidden />
            {status}
          </span>
        </span>
      </button>
      {open && (
        <div
          id={menuId}
          className="agent-switch-menu"
          role="menu"
          aria-label="Your agents"
        >
          <p className="agent-switch-heading" aria-hidden>
            Your agents
          </p>
          {companions.map((item) => {
            const active = item.id === agent.id;
            return (
              <button
                key={item.id}
                type="button"
                role="menuitemradio"
                aria-checked={active}
                className="agent-switch-item"
                onClick={() => choose(item.id)}
              >
                <AgentAvatar id={item.id} name={item.name} size={28} />
                <span className="agent-switch-text">
                  <span className="agent-switch-item-name">{item.name}</span>
                  <span className="agent-switch-item-status">
                    {!online
                      ? 'Offline'
                      : active
                        ? status
                        : item.status === 'Running'
                          ? 'Working'
                          : 'Ready'}
                  </span>
                </span>
                {active && (
                  <svg
                    className="agent-switch-check"
                    width={14}
                    height={14}
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth={2}
                    aria-hidden
                  >
                    <path d="m5 12 5 5 9-10" />
                  </svg>
                )}
              </button>
            );
          })}
          <div className="agent-switch-rule" role="separator" />
          <button
            type="button"
            role="menuitem"
            className="agent-switch-item is-action"
            onClick={() => {
              setOpen(false);
              onOpenSettings();
            }}
          >
            <GearIcon size={14} />
            <span>{agent.name} settings</span>
          </button>
        </div>
      )}
    </div>
  );
}

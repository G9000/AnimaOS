import { useEffect, useId, useRef, useState } from 'react';
import type { AgentDetail } from '../lib/types';
import { companionAgents } from '../lib/active-agent';
import { AgentAvatar } from './AgentAvatar';
import { HaloOrb } from './halo/HaloOrb';
import { ChevronIcon, GearIcon, PlusIcon } from './icons';

const AVATAR_ACCENTS = ['#E94DAB', '#4FD1C5', '#F2B44B'];

/** The header names who you are talking with: avatar, name, and what it is
 *  doing. With more than one companion it opens a list to switch. */
export function AgentSwitcher({
  agent,
  agents,
  online,
  status,
  busy,
  onSelect,
  onNewAgent,
  onManageAgents,
  locked = false,
}: {
  agent: AgentDetail;
  agents: readonly AgentDetail[];
  online: boolean;
  /** What the agent is doing now, from its presence. */
  status: string;
  /** The agent is doing something. */
  busy: boolean;
  onSelect?: (agentId: string) => void;
  onNewAgent: () => void;
  onManageAgents: () => void;
  locked?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const menuId = useId();
  const companions = companionAgents(agents);
  const accentFor = (id: string) => {
    const index = Math.max(0, companions.findIndex((item) => item.id === id));
    return AVATAR_ACCENTS[index % AVATAR_ACCENTS.length];
  };

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
        <AgentAvatar
          id={agent.id}
          name={agent.name}
          size={32}
          fallback={
            <HaloOrb accent={accentFor(agent.id)} energy={0.6} className="agent-switch-orb" />
          }
        />
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
            const rowStatus = !online ? 'Offline' : active ? status
              : item.status === 'Running' ? 'Working'
                : item.status === 'Failed' ? 'Needs attention'
                  : item.status === 'Terminated' ? 'Stopped' : 'Ready';
            const state = !online ? 'offline'
              : rowStatus === 'Waiting for your OK' || rowStatus === 'Needs attention' ? 'attention'
                : (active ? busy : item.status === 'Running') ? 'working' : 'ready';
            const description = rowStatus === 'Ready' ? item.bio?.trim() : null;
            return (
              <button
                key={item.id}
                type="button"
                role="menuitemradio"
                aria-checked={active}
                className="agent-switch-item"
                onClick={() => choose(item.id)}
              >
                <AgentAvatar
                  id={item.id}
                  name={item.name}
                  size={30}
                  fallback={<HaloOrb accent={accentFor(item.id)} energy={0.5} className="agent-switch-orb" />}
                />
                <span className="agent-switch-text">
                  <span className="agent-switch-item-name">{item.name}</span>
                  <span className="agent-switch-item-status">
                    <i className="agent-switch-status-dot" data-state={state} aria-hidden />
                    <span>{rowStatus}{description ? ` · ${description}` : ''}</span>
                  </span>
                </span>
                {active && (
                  <svg
                    className="agent-switch-check"
                    width={15}
                    height={15}
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth={2}
                    aria-hidden
                  >
                    <path d="m20 6-11 11-5-5" />
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
            disabled={locked}
            onClick={() => {
              setOpen(false);
              onNewAgent();
            }}
          >
            <PlusIcon size={14} />
            <span>New agent</span>
          </button>
          <button
            type="button"
            role="menuitem"
            className="agent-switch-item is-action"
            disabled={locked}
            onClick={() => {
              setOpen(false);
              onManageAgents();
            }}
          >
            <GearIcon size={14} />
            <span>Manage agents</span>
          </button>
        </div>
      )}
    </div>
  );
}

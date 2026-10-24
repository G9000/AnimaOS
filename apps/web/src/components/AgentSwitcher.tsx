import { useEffect, useId, useRef, useState, type RefObject } from 'react';
import type { AgentDetail } from '../lib/types';
import { companionAgents } from '../lib/active-agent';
import { AgentAvatar } from './AgentAvatar';
import { HaloOrb } from './halo/HaloOrb';
import { presenceLook } from './halo/HaloBackdrop';
import type { PresenceMode } from '../lib/presence';
import type { AppearanceAccent } from '../lib/appearance-preferences';
import {
  BrainIcon,
  ChevronIcon,
  DownloadIcon,
  GearIcon,
  HistoryIcon,
  PlusIcon,
} from './icons';

const AVATAR_ACCENTS = [
  '#E94DAB',
  '#4FD1C5',
  '#F2B44B',
  '#8C93FF',
  '#7FCB8A',
  '#FF6B5B',
];

/** Coins shown in the stack before the rest collapse into "+N". */
const MAX_COINS = 4;

type Open = 'menu' | 'agents' | null;

/** The header names who you are talking with: a ringed avatar, the name and
 *  what it is doing, then the other agents as a stack of coins. The name opens
 *  the agent's menu (other conversations, import, memory, settings); the
 *  coins open the list that switches agent. */
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
  title,
  subtitle,
  needsYou = false,
  presence = 'calm',
  appearanceAccent = 'pink',
  onOpenConversations,
  onImport,
  onOpenMemory,
  onOpenSettings,
  menuTriggerRef,
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
  /** A side thread's title, shown in place of the agent's name. */
  title?: string;
  /** What the thread is, under its title. */
  subtitle?: string;
  /** Something is waiting on the owner: a mark on the avatar. */
  needsYou?: boolean;
  /** What the agent is doing: its avatar orb moves and glows to match. */
  presence?: PresenceMode;
  appearanceAccent?: AppearanceAccent;
  onOpenConversations?: () => void;
  onImport?: () => void;
  onOpenMemory?: () => void;
  onOpenSettings?: () => void;
  /** The name button, so the opener of a panel can take focus back. */
  menuTriggerRef?: RefObject<HTMLButtonElement | null>;
}) {
  const [open, setOpen] = useState<Open>(null);
  const root = useRef<HTMLDivElement>(null);
  const ownTrigger = useRef<HTMLButtonElement>(null);
  const trigger = menuTriggerRef ?? ownTrigger;
  const stackTrigger = useRef<HTMLButtonElement>(null);
  const menuId = useId();
  const agentsId = useId();
  const companions = companionAgents(agents);
  const others = companions.filter((item) => item.id !== agent.id);
  const accentFor = (id: string) => {
    const index = Math.max(
      0,
      companions.findIndex((item) => item.id === id),
    );
    return AVATAR_ACCENTS[index % AVATAR_ACCENTS.length];
  };
  const thread = title !== undefined;
  const look = presenceLook(presence, appearanceAccent);
  const coins = others.slice(0, MAX_COINS);
  const hidden = others.length - coins.length;

  useEffect(() => {
    if (!open) return;
    const close = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(null);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      const from = open === 'menu' ? trigger : stackTrigger;
      setOpen(null);
      from.current?.focus();
    };
    document.addEventListener('pointerdown', close);
    document.addEventListener('keydown', escape);
    return () => {
      document.removeEventListener('pointerdown', close);
      document.removeEventListener('keydown', escape);
    };
  }, [open, trigger]);

  const choose = (id: string) => {
    setOpen(null);
    stackTrigger.current?.focus();
    if (id !== agent.id) onSelect?.(id);
  };
  const act = (action: (() => void) | undefined) => () => {
    setOpen(null);
    action?.();
  };
  const menuItems = [
    onOpenConversations && {
      label: 'Other conversations',
      icon: <HistoryIcon size={14} />,
      run: onOpenConversations,
    },
    onImport && {
      label: 'Import a conversation',
      icon: <DownloadIcon size={14} />,
      run: onImport,
    },
    onOpenMemory && {
      label: `What ${agent.name} remembers`,
      icon: <BrainIcon size={14} />,
      run: onOpenMemory,
    },
    onOpenSettings && {
      label: `${agent.name}'s settings`,
      icon: <GearIcon size={14} />,
      run: onOpenSettings,
    },
  ].filter((item): item is NonNullable<typeof item> => Boolean(item));

  return (
    <div ref={root} className="agent-switch">
      <h1 className="sr-only">{agent.name}</h1>
      <div className="agent-switch-who">
        <button
          ref={trigger}
          type="button"
          className="agent-switch-trigger"
          aria-label={`${agent.name}, menu`}
          aria-haspopup="menu"
          aria-expanded={open === 'menu'}
          aria-controls={open === 'menu' ? menuId : undefined}
          onClick={() => setOpen((value) => (value === 'menu' ? null : 'menu'))}
        >
          <span className={`agent-switch-ring${thread ? ' is-thread' : ''}`}>
            <AgentAvatar
              id={agent.id}
              name={agent.name}
              size={30}
              fallback={
                <HaloOrb
                  accent={thread ? accentFor(agent.id) : look.accent}
                  energy={thread ? 0.6 : look.energy}
                  className="agent-switch-orb"
                />
              }
            />
            {needsYou && <i className="agent-switch-notch" aria-hidden />}
          </span>
          <span className="agent-switch-text">
            <span className="agent-switch-name">
              <span className="scenic-wordmark" title={title ?? agent.name}>
                {title ?? agent.name}
              </span>
              <ChevronIcon size={12} />
            </span>
            <span
              className={`companion-status ${online ? 'is-online' : 'is-offline'}${busy && !thread ? ' is-busy' : ''}`}
              role="status"
            >
              {thread ? subtitle : status}
            </span>
          </span>
        </button>
        {open === 'menu' && (
          <div
            id={menuId}
            className="agent-menu"
            role="menu"
            aria-label={`${agent.name} menu`}
          >
            {menuItems.map((item, index) => (
              <button
                key={item.label}
                type="button"
                role="menuitem"
                className="agent-menu-item"
                data-first={index === 0 || undefined}
                onClick={act(item.run)}
              >
                {item.icon}
                <span>{item.label}</span>
              </button>
            ))}
          </div>
        )}
      </div>
      <i className="agent-switch-divider" aria-hidden />
      <div className="agent-switch-others">
        <button
          ref={stackTrigger}
          type="button"
          className="agent-stack"
          aria-label={`${agent.name}, switch agent`}
          aria-haspopup="menu"
          aria-expanded={open === 'agents'}
          aria-controls={open === 'agents' ? agentsId : undefined}
          onClick={() =>
            setOpen((value) => (value === 'agents' ? null : 'agents'))
          }
        >
          {coins.map((item) => (
            <span key={item.id} className="agent-coin" title={item.name}>
              <AgentAvatar
                id={item.id}
                name={item.name}
                size={20}
                fallback={
                  <HaloOrb
                    accent={accentFor(item.id)}
                    energy={0.4}
                    className="agent-switch-orb"
                  />
                }
              />
            </span>
          ))}
          {hidden > 0 && (
            <span className="agent-coin is-more">+{hidden}</span>
          )}
          {others.length === 0 && (
            <span className="agent-coin is-more">
              <PlusIcon size={11} />
            </span>
          )}
        </button>
        {open === 'agents' && (
          <div
            id={agentsId}
            className="agent-switch-menu"
            role="menu"
            aria-label="Your agents"
          >
            <p className="agent-switch-heading" aria-hidden>
              Your agents
            </p>
            {companions.map((item) => {
              const active = item.id === agent.id;
              const rowStatus = !online
                ? 'Offline'
                : active
                  ? status
                  : item.status === 'Running'
                    ? 'Working'
                    : item.status === 'Failed'
                      ? 'Needs attention'
                      : item.status === 'Terminated'
                        ? 'Stopped'
                        : 'Ready';
              const state = !online
                ? 'offline'
                : rowStatus === 'Waiting for your OK' ||
                    rowStatus === 'Needs attention'
                  ? 'attention'
                  : (active ? busy : item.status === 'Running')
                    ? 'working'
                    : 'ready';
              const description =
                rowStatus === 'Ready' ? item.bio?.trim() : null;
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
                    fallback={
                      <HaloOrb
                        accent={accentFor(item.id)}
                        energy={0.5}
                        className="agent-switch-orb"
                      />
                    }
                  />
                  <span className="agent-switch-text">
                    <span className="agent-switch-item-name">{item.name}</span>
                    <span className="agent-switch-item-status">
                      <i
                        className="agent-switch-status-dot"
                        data-state={state}
                        aria-hidden
                      />
                      <span>
                        {rowStatus}
                        {description ? ` · ${description}` : ''}
                      </span>
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
              onClick={act(onNewAgent)}
            >
              <PlusIcon size={14} />
              <span>New agent</span>
            </button>
            <button
              type="button"
              role="menuitem"
              className="agent-switch-item is-action"
              disabled={locked}
              onClick={act(onManageAgents)}
            >
              <GearIcon size={14} />
              <span>Manage agents</span>
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

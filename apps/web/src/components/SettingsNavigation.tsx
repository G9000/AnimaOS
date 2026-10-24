import type { ReactNode, RefObject } from 'react';
import type { Navigate } from '../lib/hash-route';
import type { AgentDetail } from '../lib/types';
import type { AvailablePage } from './WorkspaceShell';
import { isMacPlatform } from '../lib/platform';
import { SettingsAgentPicker } from './SettingsAgentPicker';
import { PenIcon } from './PenIcon';

interface Destination {
  page: AvailablePage;
  label: string;
  icon: ReactNode;
}

interface DestinationGroup {
  label: string;
  items: Destination[];
}

/** Settings are grouped by what the owner is deciding, not by daemon feature. */
const DESTINATION_GROUPS: DestinationGroup[] = [
  {
    label: 'Agent',
    items: [
      {
        page: 'profile',
        label: 'Profile',
        icon: <PenIcon name="user-circle-fill" size={14} />,
      },
      {
        page: 'model',
        label: 'Model',
        icon: <PenIcon name="cpu-fill" size={14} />,
      },
      {
        page: 'approvals',
        label: 'Permissions',
        icon: <PenIcon name="shield-check-fill" size={14} />,
      },
    ],
  },
  {
    label: 'Learning',
    items: [
      {
        page: 'skills',
        label: 'Skills',
        icon: <PenIcon name="lightbulb-fill" size={14} />,
      },
      {
        page: 'memory',
        label: 'Memory',
        icon: <PenIcon name="notebook-fill" size={14} />,
      },
    ],
  },
  {
    label: 'Abilities',
    items: [
      {
        page: 'capabilities',
        label: 'Tools',
        icon: <PenIcon name="wrench-fill" size={14} />,
      },
      {
        page: 'connectors',
        label: 'Connections',
        icon: <PenIcon name="plugs-connected-fill" size={14} />,
      },
    ],
  },
  {
    label: 'Activity',
    items: [
      {
        page: 'automations',
        label: 'Automations',
        icon: <PenIcon name="repeat-fill" size={14} />,
      },
      {
        page: 'work',
        label: 'Work',
        icon: <PenIcon name="list-checks-fill" size={14} />,
      },
      {
        page: 'files',
        label: 'Files',
        icon: <PenIcon name="folder-fill" size={14} />,
      },
    ],
  },
  {
    label: 'App · all agents',
    items: [
      {
        page: 'appearance',
        label: 'Appearance',
        icon: <PenIcon name="palette-fill" size={14} />,
      },
      {
        page: 'runtime',
        label: 'Runtime',
        icon: <PenIcon name="hard-drives-fill" size={14} />,
      },
      {
        page: 'usage',
        label: 'Usage',
        icon: <PenIcon name="chart-bar-fill" size={14} />,
      },
      {
        page: 'health',
        label: 'Diagnostics',
        icon: <PenIcon name="heartbeat-fill" size={14} />,
      },
      {
        page: 'logs',
        label: 'Logs',
        icon: <PenIcon name="scroll-fill" size={14} />,
      },
    ],
  },
];
export const SETTINGS_DESTINATIONS = DESTINATION_GROUPS.flatMap(
  (group) => group.items,
);

/** Whose a page is, and what it is for, under its title. */
export const SETTINGS_PAGE_META: Partial<
  Record<
    AvailablePage,
    {
      scope: 'agent' | 'all';
      description: (
        agent: string,
        mainAgent?: string,
        isHelper?: boolean,
      ) => string;
    }
  >
> = {
  profile: {
    scope: 'agent',
    description: (agent, _mainAgent, isHelper) =>
      isHelper
        ? `Who ${agent} is, what it's for, and what it can touch.`
        : `Who ${agent} is, what it thinks with, and how it sounds.`,
  },
  model: {
    scope: 'agent',
    description: (agent) =>
      `What powers ${agent}'s thinking, and what to fall back on. Each agent has its own.`,
  },
  approvals: {
    scope: 'agent',
    description: (agent) =>
      `What ${agent} can do on its own, and when it has to ask you first.`,
  },
  skills: {
    scope: 'agent',
    description: (agent) =>
      `What ${agent} has learned to do. It writes skills from experience and improves them as it uses them.`,
  },
  memory: {
    scope: 'agent',
    description: (agent) =>
      `What ${agent} remembers about you. Edit or forget anything.`,
  },
  capabilities: {
    scope: 'agent',
    description: (agent) =>
      `Everything ${agent} can use: built-in tool groups, and tools from servers you connect.`,
  },
  connectors: {
    scope: 'agent',
    description: (agent) =>
      `Services ${agent} can work with, and the chat apps it can reach you on. Set up per agent.`,
  },
  automations: {
    scope: 'agent',
    description: (agent) =>
      `Things ${agent} does on a schedule, without being asked. Everything lands in the main chat.`,
  },
  work: {
    scope: 'all',
    description: (_agent, mainAgent = 'anima') =>
      `Tasks ${mainAgent} is tracking, across every agent.`,
  },
  files: {
    scope: 'agent',
    description: (agent) => `Everything in ${agent}'s workspace folder.`,
  },
  appearance: {
    scope: 'all',
    description: () =>
      'How anima looks on this computer. Your chats and settings stay the same.',
  },
  runtime: {
    scope: 'all',
    description: (_agent, mainAgent = 'anima') =>
      `Where ${mainAgent} lives, and where its commands run.`,
  },
  usage: {
    scope: 'all',
    description: (_agent, mainAgent = 'anima') =>
      `What ${mainAgent} costs, by day, by agent and by chat.`,
  },
  health: {
    scope: 'all',
    description: () => `How the daemon is doing, and what it's saying.`,
  },
  logs: {
    scope: 'all',
    description: () => `What the daemon is saying, live.`,
  },
};

export function DestinationNavigation({
  page,
  navigate,
  placement,
  pendingApprovals,
  pendingSkills = 0,
  locked,
  query = '',
}: {
  page: AvailablePage | null;
  navigate: Navigate;
  placement: 'sidebar' | 'bottom-dock';
  pendingApprovals: number;
  pendingSkills?: number;
  locked: boolean;
  query?: string;
}) {
  const sidebar = placement === 'sidebar';
  const search = query.trim().toLowerCase();
  const groups = DESTINATION_GROUPS.map((group) => ({
    ...group,
    items: group.items.filter(
      (item) =>
        group.label.toLowerCase().includes(search) ||
        item.label.toLowerCase().includes(search),
    ),
  })).filter((group) => group.items.length > 0);
  const itemClass = `studio-nav-item ${sidebar ? 'is-row' : 'is-tab'}`;
  const destination = (item: Destination) => {
    const waiting =
      item.page === 'approvals'
        ? pendingApprovals
        : item.page === 'skills'
          ? pendingSkills
          : 0;
    return (
      <button
        key={item.page}
        type="button"
        disabled={locked}
        onClick={() => navigate({ kind: 'page', page: item.page })}
        aria-current={page === item.page ? 'page' : undefined}
        aria-label={
          waiting > 0 ? `${item.label}, ${waiting} waiting` : item.label
        }
        className={itemClass}
      >
        {item.icon}
        <span>{item.label}</span>
        {waiting > 0 && (
          <span className="nav-badge" aria-hidden>
            {waiting}
          </span>
        )}
      </button>
    );
  };
  return (
    <nav
      aria-label="Workspace navigation"
      aria-orientation={sidebar ? 'vertical' : 'horizontal'}
      data-placement={placement}
      className={
        sidebar
          ? 'studio-navigation'
          : 'safe-bottom-dock glass-strong absolute inset-x-0 bottom-0 z-30 flex items-center gap-1 overflow-x-auto p-1.5'
      }
    >
      {groups.length === 0 && (
        <p role="status" className="settings-search-empty">
          No settings found.
        </p>
      )}
      {groups.map((group) => (
        <div
          key={group.label}
          role="group"
          aria-label={group.label}
          className="contents"
        >
          {sidebar && <p className="studio-nav-group">{group.label}</p>}
          {group.items.map((item) => destination(item))}
        </div>
      ))}
    </nav>
  );
}

export function SettingsSidebar({
  agent,
  agents,
  onSelectAgent,
  onCreateAgent,
  searchRef,
  query,
  onQueryChange,
  page,
  navigate,
  pendingApprovals,
  pendingSkills = 0,
  locked,
}: {
  agent: AgentDetail;
  agents: readonly AgentDetail[];
  onSelectAgent?: (agentId: string) => void;
  onCreateAgent?: () => void;
  searchRef: RefObject<HTMLInputElement | null>;
  query: string;
  onQueryChange: (query: string) => void;
  page: AvailablePage | null;
  navigate: Navigate;
  pendingApprovals: number;
  pendingSkills?: number;
  locked: boolean;
}) {
  return (
    <aside className="studio-sidebar settings-window-nav">
      <p className="settings-window-title">Settings</p>
      <SettingsAgentPicker
        agent={agent}
        agents={agents}
        onSelect={onSelectAgent}
        onCreateAgent={onCreateAgent}
        locked={locked}
      />
      <label className="settings-search">
        <PenIcon name="magnifying-glass-fill" size={12} />
        <input
          ref={searchRef}
          type="search"
          aria-label="Search settings"
          placeholder="Search settings"
          value={query}
          disabled={locked}
          onChange={(event) => onQueryChange(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Escape' && query) {
              event.preventDefault();
              event.stopPropagation();
              onQueryChange('');
            }
          }}
        />
        <kbd aria-hidden>{isMacPlatform() ? '⌘K' : 'Ctrl K'}</kbd>
      </label>
      <DestinationNavigation
        page={page}
        navigate={navigate}
        pendingApprovals={pendingApprovals}
        pendingSkills={pendingSkills}
        locked={locked}
        placement="sidebar"
        query={query}
      />
    </aside>
  );
}

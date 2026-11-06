import { useId, useState, type ReactNode, type RefObject } from 'react';
import { useResetOnChange } from '../hooks/useResetOnChange';
import type { Navigate } from '../lib/hash-route';
import type { AgentDetail } from '../lib/types';
import type { AvailablePage } from './WorkspaceShell';
import { isMacPlatform } from '../lib/platform';
import { SettingsAgentPicker } from './SettingsAgentPicker';
import { PenIcon } from './PenIcon';
import { BellIcon } from './icons';

interface Destination {
  page: AvailablePage;
  label: string;
  icon: ReactNode;
}

interface DestinationGroup {
  label: string;
  advanced?: boolean;
  items: Destination[];
}

/** Settings are grouped by what the owner is deciding, not by daemon feature. */
const DESTINATION_GROUPS: DestinationGroup[] = [
  {
    label: 'Personal',
    items: [
      {
        page: 'profile',
        label: 'Profile',
        icon: <PenIcon name="user-circle-fill" size={14} />,
      },
      {
        page: 'appearance',
        label: 'Appearance',
        icon: <PenIcon name="palette-fill" size={14} />,
      },
      {
        page: 'notifications',
        label: 'Notifications',
        icon: <BellIcon size={14} />,
      },
      {
        page: 'memory',
        label: 'Memory',
        icon: <PenIcon name="notebook-fill" size={14} />,
      },
    ],
  },
  {
    label: 'Workspace',
    items: [
      {
        page: 'approvals',
        label: 'Permissions',
        icon: <PenIcon name="shield-check-fill" size={14} />,
      },
      {
        page: 'connectors',
        label: 'Connections',
        icon: <PenIcon name="plugs-connected-fill" size={14} />,
      },
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
    label: 'Advanced',
    advanced: true,
    items: [
      {
        page: 'model',
        label: 'Model',
        icon: <PenIcon name="cpu-fill" size={14} />,
      },
      {
        page: 'skills',
        label: 'Skills',
        icon: <PenIcon name="lightbulb-fill" size={14} />,
      },
      {
        page: 'capabilities',
        label: 'Tools',
        icon: <PenIcon name="wrench-fill" size={14} />,
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

function isAdvancedPage(page: AvailablePage | null): boolean {
  return DESTINATION_GROUPS.some(
    (group) => group.advanced && group.items.some((item) => item.page === page),
  );
}

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
  notifications: {
    scope: 'all',
    description: () =>
      'What anima tells you about while you are in another tab or app.',
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
  placement: 'sidebar' | 'mobile';
  pendingApprovals: number;
  pendingSkills?: number;
  locked: boolean;
  query?: string;
}) {
  const sidebar = placement === 'sidebar';
  const search = query.trim().toLowerCase();
  const advancedId = useId();
  const [advancedOpen, setAdvancedOpen] = useState(() => isAdvancedPage(page));
  useResetOnChange([page], () => {
    if (isAdvancedPage(page)) setAdvancedOpen(true);
  });
  const groups = DESTINATION_GROUPS.map((group) => ({
    ...group,
    items: group.items.filter(
      (item) =>
        group.label.toLowerCase().includes(search) ||
        item.label.toLowerCase().includes(search),
    ),
  })).filter((group) => group.items.length > 0);
  const waitingFor = (item: Destination) =>
    item.page === 'approvals'
      ? pendingApprovals
      : item.page === 'skills'
        ? pendingSkills
        : 0;
  if (!sidebar) {
    return (
      <nav
        aria-label="Workspace navigation"
        data-placement="mobile"
        className="settings-mobile-navigation"
      >
        <label>
          <span className="sr-only">Settings section</span>
          <select
            value={
              SETTINGS_DESTINATIONS.some((item) => item.page === page)
                ? page!
                : ''
            }
            disabled={locked}
            onChange={(event) => {
              const item = SETTINGS_DESTINATIONS.find(
                (candidate) => candidate.page === event.target.value,
              );
              if (item) navigate({ kind: 'page', page: item.page });
            }}
          >
            <option value="" disabled>
              Choose a section
            </option>
            {DESTINATION_GROUPS.map((group) => (
              <optgroup key={group.label} label={group.label}>
                {group.items.map((item) => (
                  <option key={item.page} value={item.page}>
                    {item.label}
                    {waitingFor(item) > 0
                      ? ` · ${waitingFor(item)} waiting`
                      : ''}
                  </option>
                ))}
              </optgroup>
            ))}
          </select>
        </label>
      </nav>
    );
  }
  const destination = (item: Destination) => {
    const waiting = waitingFor(item);
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
        className="studio-nav-item is-row"
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
      aria-orientation="vertical"
      data-placement={placement}
      className="studio-navigation"
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
          className="settings-navigation-group"
        >
          {group.advanced && !search ? (
            <button
              type="button"
              className="settings-advanced-toggle"
              disabled={locked}
              aria-label={
                pendingSkills > 0
                  ? `Advanced, ${pendingSkills} waiting`
                  : 'Advanced'
              }
              aria-expanded={advancedOpen}
              aria-controls={advancedId}
              onClick={() => setAdvancedOpen((open) => !open)}
            >
              <span>Advanced</span>
              {pendingSkills > 0 && (
                <span className="nav-badge" aria-hidden>
                  {pendingSkills}
                </span>
              )}
              <PenIcon name="caret-down-fill" size={12} />
            </button>
          ) : (
            <p className="studio-nav-group">{group.label}</p>
          )}
          <div
            id={group.advanced ? advancedId : undefined}
            hidden={group.advanced && !advancedOpen && !search}
          >
            {group.items.map((item) => destination(item))}
          </div>
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

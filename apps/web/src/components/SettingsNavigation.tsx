import type { ReactNode, RefObject } from 'react';
import type { Navigate } from '../lib/hash-route';
import type { AvailablePage } from './WorkspaceShell';
import { isMacPlatform } from '../lib/platform';
import {
  UserRoundIcon,
  PuzzleIcon,
  ChipIcon,
  ShieldCheckIcon,
  BrainIcon,
  BlocksIcon,
  LinkIcon,
  CalendarClockIcon,
  ListChecksIcon,
  FolderIcon,
  ServerIcon,
  ChartIcon,
  PulseIcon,
  ScrollTextIcon,
  SearchIcon,
} from './icons';

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
      { page: 'profile', label: 'Profile', icon: <UserRoundIcon size={15} /> },
      { page: 'model', label: 'Model', icon: <ChipIcon size={15} /> },
      {
        page: 'approvals',
        label: 'Permissions',
        icon: <ShieldCheckIcon size={15} />,
      },
    ],
  },
  {
    label: 'Learning',
    items: [
      { page: 'skills', label: 'Skills', icon: <PuzzleIcon size={15} /> },
      { page: 'memory', label: 'Memory', icon: <BrainIcon size={15} /> },
    ],
  },
  {
    label: 'Abilities',
    items: [
      { page: 'capabilities', label: 'Tools', icon: <BlocksIcon size={15} /> },
      {
        page: 'connectors',
        label: 'Connections',
        icon: <LinkIcon size={15} />,
      },
    ],
  },
  {
    label: 'Activity',
    items: [
      {
        page: 'automations',
        label: 'Automations',
        icon: <CalendarClockIcon size={15} />,
      },
      { page: 'work', label: 'Work', icon: <ListChecksIcon size={15} /> },
      { page: 'files', label: 'Files', icon: <FolderIcon size={15} /> },
    ],
  },
  {
    label: 'System',
    items: [
      { page: 'runtime', label: 'Runtime', icon: <ServerIcon size={15} /> },
      { page: 'usage', label: 'Usage', icon: <ChartIcon size={15} /> },
      { page: 'health', label: 'Diagnostics', icon: <PulseIcon size={15} /> },
      { page: 'logs', label: 'Logs', icon: <ScrollTextIcon size={15} /> },
    ],
  },
];
export const SETTINGS_DESTINATIONS = DESTINATION_GROUPS.flatMap(
  (group) => group.items,
);

export function DestinationNavigation({
  page,
  navigate,
  placement,
  pendingApprovals,
  locked,
  query = '',
}: {
  page: AvailablePage | null;
  navigate: Navigate;
  placement: 'sidebar' | 'bottom-dock';
  pendingApprovals: number;
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
    const waiting = item.page === 'approvals' ? pendingApprovals : 0;
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
  searchRef,
  query,
  onQueryChange,
  ...navigation
}: {
  searchRef: RefObject<HTMLInputElement | null>;
  query: string;
  onQueryChange: (query: string) => void;
  page: AvailablePage | null;
  navigate: Navigate;
  pendingApprovals: number;
  locked: boolean;
}) {
  return (
    <aside className="studio-sidebar settings-window-nav">
      <p className="settings-window-title">Settings</p>
      <label className="settings-search">
        <SearchIcon size={13} />
        <input
          ref={searchRef}
          type="search"
          aria-label="Search settings"
          placeholder="Search settings"
          value={query}
          disabled={navigation.locked}
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
        {...navigation}
        placement="sidebar"
        query={query}
      />
    </aside>
  );
}

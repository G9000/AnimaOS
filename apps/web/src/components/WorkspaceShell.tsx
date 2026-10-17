import { useEffect, useRef, useState, type ReactNode } from 'react';
import type { Session } from '@animaOS-SWARM/sdk';
import type { DaemonConnection } from '../hooks/useDaemonBootstrap';
import type { DaemonWorkspaceState } from '../lib/daemon-api';
import type { HashPage, HashRoute, Navigate } from '../lib/hash-route';
import type { AgentDetail } from '../lib/types';
import { AgentPresence } from './AgentPresence';
import { WorkspaceHub } from './WorkspaceHub';
import { WorkspaceFiles } from './WorkspaceFiles';
import { WorkspaceCapabilities } from './WorkspaceCapabilities';
import { CommandMenu, type StudioCommand } from './CommandMenu';
import { PROMPT_LIBRARY } from '../lib/prompt-library';
import { sessionKey } from '../lib/session-groups';
import {
  AgentsIcon,
  BoltIcon,
  ChartIcon,
  ChipIcon,
  ChevronIcon,
  ClockIcon,
  GearIcon,
  HeartIcon,
  LogIcon,
  PlusIcon,
  PulseIcon,
  ShieldIcon,
  SparkIcon,
  XIcon,
} from './icons';
import { ghostBtnCls } from './ui-bits';

/** Pages this release renders; the other hash pages open the conversation
 *  until their milestones build them. */
export const AVAILABLE_PAGES = [
  'profile',
  'model',
  'runtime',
  'approvals',
  'automations',
  'memory',
  'skills',
  'work',
  'files',
  'connectors',
  'usage',
  'logs',
  'health',
  'capabilities',
] as const satisfies readonly HashPage[];
export type AvailablePage = (typeof AVAILABLE_PAGES)[number];

/** Session titles the command menu offers, newest activity first. */
export const MAX_SESSION_COMMANDS = 50;

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
    label: 'Companion',
    items: [
      { page: 'profile', label: 'Profile', icon: <AgentsIcon size={15} /> },
      { page: 'model', label: 'Model', icon: <ChipIcon size={15} /> },
      {
        page: 'approvals',
        label: 'Permissions',
        icon: <ShieldIcon size={15} />,
      },
    ],
  },
  {
    label: 'Learning',
    items: [
      { page: 'skills', label: 'Skills', icon: <BoltIcon size={15} /> },
      { page: 'memory', label: 'Memory', icon: <PulseIcon size={15} /> },
    ],
  },
  {
    label: 'Abilities',
    items: [
      { page: 'capabilities', label: 'Tools', icon: <SparkIcon size={15} /> },
      {
        page: 'connectors',
        label: 'Connections',
        icon: <GearIcon size={15} />,
      },
    ],
  },
  {
    label: 'Activity',
    items: [
      {
        page: 'automations',
        label: 'Automations',
        icon: <ClockIcon size={15} />,
      },
      { page: 'work', label: 'Work', icon: <SparkIcon size={15} /> },
      { page: 'files', label: 'Files', icon: <LogIcon size={15} /> },
    ],
  },
  {
    label: 'System',
    items: [
      { page: 'runtime', label: 'Runtime', icon: <HeartIcon size={15} /> },
      { page: 'usage', label: 'Usage', icon: <ChartIcon size={15} /> },
      { page: 'health', label: 'Diagnostics', icon: <PulseIcon size={15} /> },
      { page: 'logs', label: 'Logs', icon: <LogIcon size={15} /> },
    ],
  },
];
const DESTINATIONS = DESTINATION_GROUPS.flatMap((group) => group.items);

export function availablePage(route: HashRoute): AvailablePage | null {
  return route.kind === 'page' &&
    (AVAILABLE_PAGES as readonly HashPage[]).includes(route.page)
    ? (route.page as AvailablePage)
    : null;
}

const ignoreWorkspaceAvatarChange = async () => undefined;
const DESKTOP_NAVIGATION_QUERY = '(min-width: 768px)';

function useDesktopNavigation() {
  const [desktop, setDesktop] = useState(
    () =>
      typeof window.matchMedia !== 'function' ||
      window.matchMedia(DESKTOP_NAVIGATION_QUERY).matches,
  );
  useEffect(() => {
    if (typeof window.matchMedia !== 'function') return;
    const media = window.matchMedia(DESKTOP_NAVIGATION_QUERY);
    const update = () => setDesktop(media.matches);
    update();
    media.addEventListener('change', update);
    return () => media.removeEventListener('change', update);
  }, []);
  return desktop;
}

function DestinationNavigation({
  page,
  navigate,
  placement,
  pendingApprovals,
  locked,
}: {
  page: AvailablePage | null;
  navigate: Navigate;
  placement: 'sidebar' | 'bottom-dock';
  pendingApprovals: number;
  locked: boolean;
}) {
  const sidebar = placement === 'sidebar';
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
      {DESTINATION_GROUPS.map((group) => (
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

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** The thread switcher: a modal panel rendered outside the inert shell. It
 *  takes focus, keeps Tab inside, and closes on Escape; the shell returns
 *  focus to the thread button. */
function SessionDrawer({
  children,
  onClose,
  onNewChat,
}: {
  children: ReactNode;
  onClose: () => void;
  onNewChat: () => void;
}) {
  const panel = useRef<HTMLDivElement>(null);
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(() => {
    const element = panel.current;
    if (!element) return;
    (element.querySelector<HTMLElement>(FOCUSABLE) ?? element).focus();
    // A control that unmounts while focused, such as a row menu item after
    // Archive, drops focus to the body; bring it back into the panel.
    const observer = new MutationObserver(() => {
      const active = document.activeElement;
      if (!active || active === document.body) element.focus();
    });
    observer.observe(element, { childList: true, subtree: true });
    // Keys are handled at the document so they work wherever focus is. A
    // control that handles its own Escape (a row menu or the rename field)
    // closes only itself.
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      if (event.key === 'Escape') {
        event.preventDefault();
        close.current();
        return;
      }
      if (event.key !== 'Tab') return;
      const focusable = Array.from(
        element.querySelectorAll<HTMLElement>(FOCUSABLE),
      );
      if (focusable.length === 0) {
        event.preventDefault();
        element.focus();
        return;
      }
      const index = focusable.indexOf(document.activeElement as HTMLElement);
      const next =
        index === -1
          ? event.shiftKey
            ? focusable.length - 1
            : 0
          : event.shiftKey && index === 0
            ? focusable.length - 1
            : !event.shiftKey && index === focusable.length - 1
              ? 0
              : null;
      if (next === null) return;
      event.preventDefault();
      focusable[next].focus();
    };
    document.addEventListener('keydown', onKeyDown);
    return () => {
      observer.disconnect();
      document.removeEventListener('keydown', onKeyDown);
    };
  }, []);
  return (
    <div
      className="session-drawer"
      role="dialog"
      aria-modal="true"
      aria-label="Sessions"
    >
      <div
        ref={panel}
        tabIndex={-1}
        className="session-drawer-panel studio-sidebar"
      >
        <div className="session-drawer-head">
          <button type="button" className={ghostBtnCls} onClick={onNewChat}>
            <PlusIcon size={14} />
            New chat
          </button>
          <button
            type="button"
            className="studio-tool-button"
            aria-label="Close sessions"
            onClick={onClose}
          >
            <XIcon size={14} />
          </button>
        </div>
        {children}
      </div>
      <div className="session-drawer-backdrop" aria-hidden onClick={onClose} />
    </div>
  );
}

function routeSession(
  route: HashRoute,
  sessions: readonly Session[],
): Session | null {
  if (route.kind !== 'session') return null;
  return (
    sessions.find(
      (session) =>
        session.id === route.sessionId &&
        (!route.agentId || session.agentId === route.agentId),
    ) ?? null
  );
}

export function WorkspaceShell({
  mainAgent,
  agents,
  connection,
  route,
  navigate,
  conversation,
  conversationRoute,
  sidebar = null,
  connectors = null,
  approvals = null,
  automations = null,
  skills = null,
  memory = null,
  usage = null,
  logs = null,
  health = null,
  pendingApprovals = 0,
  sessions = [],
  onOpenSession,
  workspaceState = null,
  profile = null,
  model = null,
  runtime = null,
  locked = false,
  onChangeWorkspaceAvatar = ignoreWorkspaceAvatarChange,
  onPickPrompt,
  onNewChat,
}: {
  mainAgent: AgentDetail;
  agents: readonly AgentDetail[];
  connection: Exclude<DaemonConnection, 'unknown'>;
  route: HashRoute;
  navigate: Navigate;
  /** The chat or session view; kept mounted so drafts and scroll survive page visits. */
  conversation: ReactNode;
  /** The chat or session a page returns to; defaults to the last one visited. */
  conversationRoute?: HashRoute;
  /** The sessions list shown by the thread switcher. */
  sidebar?: ReactNode | null;
  connectors?: ReactNode | null;
  /** The Approvals page, shown at `#/approvals` as Permissions. */
  approvals?: ReactNode | null;
  /** The Automations page, shown at `#/automations`. */
  automations?: ReactNode | null;
  /** The Skills page, shown at `#/skills`. */
  skills?: ReactNode | null;
  /** The Memory page, shown at `#/memory`. */
  memory?: ReactNode | null;
  /** The Usage page, shown at `#/usage`. */
  usage?: ReactNode | null;
  /** The Logs page, shown at `#/logs`. */
  logs?: ReactNode | null;
  /** The Health page, shown at `#/health` as Diagnostics. */
  health?: ReactNode | null;
  /** Approvals waiting for the owner, for the destination's badge. */
  pendingApprovals?: number;
  /** The listed sessions the command menu and thread button use. */
  sessions?: readonly Session[];
  onOpenSession?: (session: Session) => void;
  workspaceState?: DaemonWorkspaceState | null;
  /** The companion's own settings pages: identity, model, and runtime. */
  profile?: ReactNode | null;
  model?: ReactNode | null;
  runtime?: ReactNode | null;
  /** Holds the Settings window on its page while a save or reset settles. */
  locked?: boolean;
  onChangeWorkspaceAvatar?: (file: File) => Promise<void>;
  onPickPrompt?: (prompt: string) => void;
  onNewChat?: () => void;
}) {
  const page = availablePage(route);
  const [visitedConversation, setVisitedConversation] = useState<HashRoute>(
    route.kind === 'session' ? route : { kind: 'home' },
  );
  const lastConversation = conversationRoute ?? visitedConversation;
  const [commandsOpen, setCommandsOpen] = useState(false);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const desktopNavigation = useDesktopNavigation();
  const drawerShown = drawerOpen && sidebar !== null;
  const sessionsButtonRef = useRef<HTMLButtonElement>(null);
  const drawerWasShown = useRef(false);
  useEffect(() => {
    if (drawerWasShown.current && !drawerShown)
      sessionsButtonRef.current?.focus();
    drawerWasShown.current = drawerShown;
  }, [drawerShown]);
  const companyName = workspaceState?.configured
    ? (workspaceState.workspace?.companyName ?? null)
    : null;
  const hasAvatar =
    workspaceState?.configured === true &&
    workspaceState.workspace?.hasAvatar === true;
  const workingHelpers = agents.filter(
    (agent) => agent.id !== mainAgent.id && agent.status === 'Running',
  ).length;
  const newChat = onNewChat ?? (() => navigate({ kind: 'home' }));
  const openConversation = () => navigate(lastConversation);
  const currentSession = routeSession(lastConversation, sessions);
  const threadTitle = currentSession?.title || 'New chat';

  useEffect(() => {
    if (route.kind === 'session' || route.kind === 'home')
      setVisitedConversation(route);
    setDrawerOpen(false);
  }, [route]);

  useEffect(() => {
    const shortcut = (event: KeyboardEvent) => {
      if (
        (event.ctrlKey || event.metaKey) &&
        event.key.toLowerCase() === 'k' &&
        !event.isComposing
      ) {
        if (document.querySelector('[aria-modal="true"]')) return;
        event.preventDefault();
        setCommandsOpen(true);
      }
    };
    window.addEventListener('keydown', shortcut);
    return () => window.removeEventListener('keydown', shortcut);
  }, []);

  const commands: StudioCommand[] = [
    {
      id: 'new-chat',
      title: 'New chat',
      description: 'Start a fresh conversation',
      group: 'Navigate',
      run: newChat,
    },
    {
      id: 'review-approvals',
      title: 'Review tool approvals',
      description:
        pendingApprovals > 0
          ? `${pendingApprovals} waiting for you`
          : 'Nothing is waiting',
      group: 'Navigate',
      run: () => navigate({ kind: 'page', page: 'approvals' }),
    },
    ...DESTINATIONS.map((item) => ({
      id: item.page,
      title: `Go to ${item.label}`,
      description: `Open ${item.label.toLowerCase()}`,
      group: 'Navigate',
      run: () => navigate({ kind: 'page', page: item.page }),
    })),
    ...(onOpenSession
      ? sessions
          .filter((session) => !session.archived)
          .slice(0, MAX_SESSION_COMMANDS)
          .map((session) => ({
            id: `session:${sessionKey(session)}`,
            title: session.title,
            description: 'Open this conversation',
            group: 'Sessions',
            run: () => onOpenSession(session),
          }))
      : []),
    ...(onPickPrompt
      ? PROMPT_LIBRARY.map((prompt) => ({
          id: prompt.id,
          title: prompt.title,
          description: prompt.description,
          group: prompt.category,
          run: () => {
            if (page !== null) openConversation();
            onPickPrompt(prompt.prompt);
            requestAnimationFrame(() =>
              document
                .querySelector<HTMLTextAreaElement>('[data-workspace-composer]')
                ?.focus(),
            );
          },
        }))
      : []),
  ];

  const pageLabel = page
    ? (DESTINATIONS.find((item) => item.page === page)?.label ?? null)
    : null;

  return (
    <>
      <div
        className="studio-shell companion-shell scenic-shell relative z-[1] flex min-h-0 flex-1 flex-col"
        inert={commandsOpen || drawerShown || undefined}
        aria-hidden={commandsOpen || drawerShown || undefined}
      >
        <div className="studio-frame relative flex min-h-0 flex-1">
          <main className="studio-main workspace-mobile-safe relative min-h-0 min-w-0 flex-1">
            <div className="scenic-backdrop" aria-hidden />
            <div className="studio-topbar scenic-topbar">
              <div className="scenic-identity">
                <h1 className="scenic-wordmark" title={mainAgent.name}>
                  {mainAgent.name}
                </h1>
                <span className="scenic-divider" aria-hidden>
                  /
                </span>
                {sidebar !== null ? (
                  <button
                    ref={sessionsButtonRef}
                    type="button"
                    className="scenic-thread"
                    aria-label="Open sessions"
                    aria-expanded={drawerOpen}
                    aria-haspopup="dialog"
                    onClick={() => setDrawerOpen(true)}
                  >
                    {currentSession &&
                      currentSession.kind !== 'chat' &&
                      (currentSession.kind === 'telegram' ||
                        currentSession.kind === 'discord') && (
                        <span className="session-kind-badge">
                          {currentSession.kind === 'telegram'
                            ? 'Telegram'
                            : 'Discord'}
                        </span>
                      )}
                    <span className="scenic-thread-title">{threadTitle}</span>
                    <ChevronIcon size={13} />
                  </button>
                ) : (
                  <span className="scenic-thread">
                    <span className="scenic-thread-title">{threadTitle}</span>
                  </span>
                )}
              </div>
              <div className="companion-status" role="status">
                <span
                  className={`studio-connection ${connection === 'online' ? 'is-online' : 'is-offline'}`}
                >
                  <i aria-hidden />
                  {connection === 'online' ? 'Connected' : 'Offline'}
                </span>
                <p>
                  {connection === 'offline'
                    ? 'Cannot reach your companion'
                    : mainAgent.status === 'Running'
                      ? 'Working on your request'
                      : 'Ready when you are'}
                </p>
                <span
                  className={
                    connection === 'online' && workingHelpers === 0
                      ? 'scenic-quiet'
                      : undefined
                  }
                >
                  {connection === 'offline'
                    ? 'Check the server connection.'
                    : workingHelpers > 0
                      ? `${workingHelpers} ${workingHelpers === 1 ? 'helper is' : 'helpers are'} working`
                      : 'Your conversations stay with you.'}
                </span>
              </div>
              <div className="studio-topbar-actions">
                {pendingApprovals > 0 && (
                  <button
                    type="button"
                    className="studio-tool-button scenic-waiting"
                    aria-label={`Approvals, ${pendingApprovals} waiting`}
                    onClick={() =>
                      navigate({ kind: 'page', page: 'approvals' })
                    }
                  >
                    <i aria-hidden />
                    {pendingApprovals} waiting
                  </button>
                )}
                <button
                  type="button"
                  className="studio-tool-button"
                  onClick={newChat}
                >
                  <PlusIcon size={14} />
                  <span className="sr-only">New chat</span>
                </button>
                <button
                  type="button"
                  className="studio-command-trigger"
                  onClick={() => setCommandsOpen(true)}
                  aria-label="Open command menu"
                >
                  <span aria-hidden>⌕</span>
                  <span className="studio-command-trigger-label">Search</span>
                  <kbd>Ctrl K</kbd>
                </button>
                <button
                  type="button"
                  onClick={() => navigate({ kind: 'page', page: 'profile' })}
                  aria-current={page !== null ? 'page' : undefined}
                  className="studio-tool-button scenic-settings"
                  aria-label="Settings"
                  title={`Settings for ${mainAgent.name}`}
                >
                  <GearIcon size={15} />
                </button>
              </div>
            </div>
            <div className="studio-view">
              {/* Keep the conversation mounted so drafts and scroll survive page visits. */}
              <div className="companion-chat-panel" hidden={page !== null}>
                {conversation}
              </div>
              {page !== null && (
                <section className="settings-window" aria-label="Settings">
                  {desktopNavigation && (
                    <aside className="studio-sidebar settings-window-nav">
                      <p className="settings-window-title">Settings</p>
                      <AgentPresence
                        agent={mainAgent}
                        connection={connection}
                        companyName={companyName}
                        placement="sidebar"
                        hasAvatar={hasAvatar}
                        onChangeWorkspaceAvatar={onChangeWorkspaceAvatar}
                      />
                      <DestinationNavigation
                        page={page}
                        navigate={navigate}
                        placement="sidebar"
                        pendingApprovals={pendingApprovals}
                        locked={locked}
                      />
                    </aside>
                  )}
                  <div className="settings-window-content">
                    <div className="settings-window-head">
                      <strong>{pageLabel}</strong>
                      {locked && (
                        <span id="settings-window-locked" className="sr-only">
                          Settings stay on this page until the change finishes.
                        </span>
                      )}
                      <button
                        type="button"
                        className="studio-tool-button"
                        aria-label="Open companion chat"
                        aria-describedby={
                          locked ? 'settings-window-locked' : undefined
                        }
                        disabled={locked}
                        onClick={openConversation}
                      >
                        <span className="scenic-hide-narrow">Back to chat</span>
                        <XIcon size={14} />
                      </button>
                    </div>
                    <div className="settings-window-page">
                      {page === 'profile' ? (
                        profile
                      ) : page === 'model' ? (
                        model
                      ) : page === 'runtime' ? (
                        runtime
                      ) : page === 'approvals' ? (
                        approvals
                      ) : page === 'automations' ? (
                        automations
                      ) : page === 'memory' ? (
                        memory
                      ) : page === 'skills' ? (
                        skills
                      ) : page === 'usage' ? (
                        usage
                      ) : page === 'logs' ? (
                        logs
                      ) : page === 'health' ? (
                        health
                      ) : page === 'connectors' ? (
                        connectors
                      ) : page === 'files' ? (
                        <WorkspaceFiles online={connection === 'online'} />
                      ) : page === 'capabilities' ? (
                        <WorkspaceCapabilities
                          online={connection === 'online'}
                        />
                      ) : page === 'work' ? (
                        <WorkspaceHub
                          agents={[mainAgent]}
                          online={connection === 'online'}
                          initialSection="Tasks"
                        />
                      ) : null}
                    </div>
                    {!desktopNavigation && (
                      <DestinationNavigation
                        page={page}
                        navigate={navigate}
                        placement="bottom-dock"
                        pendingApprovals={pendingApprovals}
                        locked={locked}
                      />
                    )}
                  </div>
                </section>
              )}
            </div>
          </main>
        </div>
      </div>
      {drawerShown && (
        <SessionDrawer
          onClose={() => setDrawerOpen(false)}
          onNewChat={() => {
            setDrawerOpen(false);
            newChat();
          }}
        >
          {sidebar}
        </SessionDrawer>
      )}
      {commandsOpen && (
        <CommandMenu commands={commands} close={() => setCommandsOpen(false)} />
      )}
    </>
  );
}

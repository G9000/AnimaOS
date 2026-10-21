import { isMacPlatform } from '../lib/platform';
import { useEffect, useRef, useState, type ReactNode } from 'react';
import type {
  Session,
  SessionMessageSearchHit,
  SessionMessageSearchPage,
} from '@animaOS-SWARM/sdk';
import type { DaemonConnection } from '../hooks/useDaemonBootstrap';
import type { DaemonWorkspaceState } from '../lib/daemon-api';
import type { HashPage, HashRoute, Navigate } from '../lib/hash-route';
import type { AgentDetail } from '../lib/types';
import { AgentPresence } from './AgentPresence';
import {
  SettingsSidebar,
  DestinationNavigation,
  SETTINGS_DESTINATIONS,
} from './SettingsNavigation';
import { AgentSwitcher } from './AgentSwitcher';
import { AgentsView } from './AgentsView';
import { HaloBackdrop } from './halo/HaloBackdrop';
import {
  presenceMode,
  usePresenceSignals,
  type PresenceMode,
} from '../lib/presence';
import { WorkspaceHub } from './WorkspaceHub';
import { WorkspaceFiles } from './WorkspaceFiles';
import { WorkspaceCapabilities } from './WorkspaceCapabilities';
import { CommandMenu, type StudioCommand } from './CommandMenu';
import { PROMPT_LIBRARY } from '../lib/prompt-library';
import { sessionKey, shouldShowSession } from '../lib/session-groups';
import {
  ChevronIcon,
  GearIcon,
  HistoryIcon,
  InboxIcon,
  SearchIcon,
  XIcon,
} from './icons';
import { ImportConversationDialog } from './sessions/ImportConversationDialog';
import { onOpenInbox } from '../lib/inbox-events';

/** Pages this release renders; the other hash pages open the conversation
 *  until their milestones build them. */
export const AVAILABLE_PAGES = [
  'agents',
  'new-agent',
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

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** The thread switcher: a modal panel rendered outside the inert shell. It
 *  takes focus, keeps Tab inside, and closes on Escape; the shell returns
 *  focus to the thread button. */
function SessionDrawer({
  children,
  onClose,
  onNewChat,
  onImport,
}: {
  children: ReactNode;
  onClose: () => void;
  onNewChat: () => void;
  /** Opens the dialog that imports a Claude Code or Codex conversation. */
  onImport?: () => void;
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
      aria-label="History"
    >
      <div
        ref={panel}
        tabIndex={-1}
        className="session-drawer-panel studio-sidebar"
      >
        <div className="session-drawer-head">
          <strong className="session-drawer-title">Other conversations</strong>
          <button
            type="button"
            className="studio-tool-button"
            onClick={onNewChat}
          >
            Main chat
          </button>
          <button
            type="button"
            className="studio-tool-button"
            aria-label="Close history"
            onClick={onClose}
          >
            <XIcon size={14} />
          </button>
        </div>
        <p className="session-drawer-note">
          Your main chat is where you talk to anima. These are side threads it
          started, group chats, and conversations from before it.
        </p>
        {children}
        {onImport && (
          <button
            type="button"
            className="session-drawer-import"
            onClick={onImport}
          >
            Import from Claude Code or Codex
          </button>
        )}
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

/** What the agent is doing, in the header under its name. */
export function agentStatusText({
  online,
  presence,
  workingHelpers,
}: {
  online: boolean;
  presence: PresenceMode;
  workingHelpers: number;
}): string {
  if (!online) return 'Offline';
  if (presence === 'listening') return 'Listening';
  if (presence === 'speaking') return 'Speaking';
  if (presence === 'attention') return 'Waiting for your OK';
  const helpers =
    workingHelpers > 0
      ? `${workingHelpers} ${workingHelpers === 1 ? 'helper' : 'helpers'} working`
      : null;
  if (presence === 'thinking')
    return helpers ? `Working · ${helpers}` : 'Working on your request';
  return helpers ?? 'Ready';
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
  onSelectAgent,
  newAgent = null,
  searchMessages,
  onOpenMessage,
  inbox = null,
  onOpenReport,
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
  searchMessages?: (
    query: string,
    signal: AbortSignal,
  ) => Promise<SessionMessageSearchPage>;
  onOpenMessage?: (hit: SessionMessageSearchHit) => void;
  /** Switches the conversation to another companion. */
  onSelectAgent?: (agentId: string) => void;
  /** Existing companion setup, opened by the agent dropdown. */
  newAgent?: ReactNode;
  /** The "Needs you" inbox: how many things wait, and its panel. */
  inbox?: {
    count: number;
    render: (close: () => void) => ReactNode;
  } | null;
  /** Opens the weekly report. */
  onOpenReport?: () => void;
}) {
  const page = availablePage(route);
  const presenceSignals = usePresenceSignals();
  const presence = presenceMode({
    signals: presenceSignals,
    running: connection === 'online' && mainAgent.status === 'Running',
    waiting: pendingApprovals > 0,
  });
  const [visitedConversation, setVisitedConversation] = useState<HashRoute>(
    route.kind === 'session' ? route : { kind: 'home' },
  );
  const lastConversation = conversationRoute ?? visitedConversation;
  const [commandsOpen, setCommandsOpen] = useState(false);
  const [settingsQuery, setSettingsQuery] = useState('');
  const settingsSearchRef = useRef<HTMLInputElement>(null);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
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
    (agent) => agent.workspaceRole === 'helper' && agent.status === 'Running',
  ).length;
  const newChat = onNewChat ?? (() => navigate({ kind: 'home' }));
  const openConversation = () => navigate(lastConversation);
  const currentSession = routeSession(lastConversation, sessions);
  // One main chat (OpenClaw-style): everything else is a side thread you
  // visit and leave.
  const sideThread =
    lastConversation.kind === 'session' &&
    currentSession !== null &&
    currentSession.mainChat !== true
      ? currentSession
      : null;
  const statusText = agentStatusText({
    online: connection === 'online',
    presence,
    workingHelpers,
  });

  useEffect(() => {
    if (route.kind === 'session' || route.kind === 'home')
      setVisitedConversation(route);
    setDrawerOpen(false);
    setSettingsQuery('');
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
        if (page !== null && desktopNavigation) {
          settingsSearchRef.current?.focus();
          settingsSearchRef.current?.select();
        } else {
          setCommandsOpen(true);
        }
      }
    };
    window.addEventListener('keydown', shortcut);
    return () => window.removeEventListener('keydown', shortcut);
  }, [page, desktopNavigation]);

  const commands: StudioCommand[] = [
    {
      id: 'new-chat',
      title: 'Main chat',
      description: 'Resume your ongoing conversation',
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
    ...(onOpenReport
      ? [
          {
            id: 'weekly-report',
            title: 'Weekly report',
            description: 'What it did this week, what it cost, what went wrong',
            group: 'Navigate',
            run: onOpenReport,
          },
        ]
      : []),
    ...SETTINGS_DESTINATIONS.map((item) => ({
      id: item.page,
      title: `Go to ${item.label}`,
      description: `Open ${item.label.toLowerCase()}`,
      group: 'Navigate',
      run: () => navigate({ kind: 'page', page: item.page }),
    })),
    ...(onOpenSession
      ? sessions
          .filter((session) => !session.archived && shouldShowSession(session))
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

  const pageLabel =
    page === 'agents'
      ? 'Manage agents'
      : page === 'new-agent'
        ? 'New agent'
        : page
          ? (SETTINGS_DESTINATIONS.find((item) => item.page === page)?.label ??
            null)
          : null;

  return (
    <>
      <div
        className="studio-shell companion-shell scenic-shell relative z-[1] flex min-h-0 flex-1 flex-col"
        data-presence={page === null ? presence : 'calm'}
        inert={commandsOpen || drawerShown || undefined}
        aria-hidden={commandsOpen || drawerShown || undefined}
      >
        <div className="studio-frame relative flex min-h-0 flex-1">
          <main className="studio-main workspace-mobile-safe relative min-h-0 min-w-0 flex-1">
            <HaloBackdrop
              mode={page === null ? presence : 'calm'}
              words={presenceSignals.listening?.words}
            />
            <div className="studio-topbar scenic-topbar">
              <div className="scenic-identity">
                <AgentSwitcher
                  agent={mainAgent}
                  agents={agents}
                  online={connection === 'online'}
                  status={statusText}
                  busy={presence !== 'calm'}
                  locked={locked}
                  onSelect={onSelectAgent}
                  onNewAgent={() =>
                    navigate({ kind: 'page', page: 'new-agent' })
                  }
                  onManageAgents={() =>
                    navigate({ kind: 'page', page: 'agents' })
                  }
                />
                {sideThread && (
                  <>
                    <button
                      type="button"
                      className="scenic-thread is-back"
                      onClick={newChat}
                    >
                      <ChevronIcon size={13} className="rotate-90" />
                      <span className="scenic-thread-title">Main chat</span>
                    </button>
                    <span
                      className="scenic-side-thread"
                      title={sideThread.title}
                    >
                      {(sideThread.kind === 'telegram' ||
                        sideThread.kind === 'discord') && (
                        <span className="session-kind-badge">
                          {sideThread.kind === 'telegram'
                            ? 'Telegram'
                            : 'Discord'}
                        </span>
                      )}
                      <span className="scenic-thread-title">
                        {sideThread.title}
                      </span>
                    </span>
                  </>
                )}
              </div>
              <div className="studio-topbar-actions">
                {inbox ? (
                  <InboxButton inbox={inbox} />
                ) : (
                  pendingApprovals > 0 && (
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
                  )
                )}
                {sidebar !== null && (
                  <button
                    ref={sessionsButtonRef}
                    type="button"
                    className="studio-tool-button"
                    aria-label="History"
                    aria-expanded={drawerOpen}
                    aria-haspopup="dialog"
                    onClick={() => setDrawerOpen(true)}
                  >
                    <HistoryIcon size={14} />
                    <span className="scenic-hide-narrow" aria-hidden>
                      History
                    </span>
                  </button>
                )}
                <button
                  type="button"
                  className="studio-command-trigger"
                  onClick={() => setCommandsOpen(true)}
                  aria-label="Open command menu"
                >
                  <SearchIcon size={14} />
                  <span className="studio-command-trigger-label">
                    Search or do anything
                  </span>
                  <kbd>{isMacPlatform() ? '⌘K' : 'Ctrl K'}</kbd>
                </button>
                <button
                  type="button"
                  onClick={() => navigate({ kind: 'page', page: 'profile' })}
                  aria-current={page !== null ? 'page' : undefined}
                  className="studio-tool-button scenic-settings"
                  aria-label="Settings"
                  title={`Settings for ${mainAgent.name}`}
                >
                  <GearIcon size={14} />
                </button>
              </div>
            </div>
            <div className="studio-view">
              {/* Keep the conversation mounted so drafts and scroll survive page visits. */}
              <div className="companion-chat-panel" hidden={page !== null}>
                {conversation}
              </div>
              {page !== null && (
                <section
                  className="settings-window"
                  data-page={page}
                  aria-label="Settings"
                >
                  {desktopNavigation && (
                    <SettingsSidebar
                      page={page}
                      navigate={navigate}
                      pendingApprovals={pendingApprovals}
                      locked={locked}
                      searchRef={settingsSearchRef}
                      query={settingsQuery}
                      onQueryChange={setSettingsQuery}
                    />
                  )}
                  <div className="settings-window-content">
                    <div className="settings-window-head">
                      <div>
                        <strong>{pageLabel}</strong>
                        {page === 'profile' && (
                          <p className="settings-profile-description">
                            Who {mainAgent.name} is, what it thinks with, and
                            how it sounds.
                          </p>
                        )}
                      </div>
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
                        <span className="scenic-hide-narrow">
                          {page === 'profile' ? 'ESC' : 'Back to chat'}
                        </span>
                        <XIcon size={14} />
                      </button>
                    </div>
                    <div className="settings-window-page">
                      {page === 'new-agent' ? (
                        newAgent
                      ) : page === 'agents' ? (
                        <AgentsView
                          agents={agents}
                          mainAgent={mainAgent}
                          onSelectAgent={(id) => {
                            onSelectAgent?.(id);
                            openConversation();
                          }}
                        />
                      ) : page === 'profile' ? (
                        <>
                          {profile}
                          <details className="settings-workspace-identity">
                            <summary>Workspace identity</summary>
                            <AgentPresence
                              agent={mainAgent}
                              connection={connection}
                              companyName={companyName}
                              placement="sidebar"
                              hasAvatar={hasAvatar}
                              onChangeWorkspaceAvatar={onChangeWorkspaceAvatar}
                            />
                          </details>
                        </>
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
          onImport={() => {
            setDrawerOpen(false);
            setImportOpen(true);
          }}
        >
          {sidebar}
        </SessionDrawer>
      )}
      {importOpen && (
        <ImportConversationDialog
          agentId={mainAgent.id}
          agentName={mainAgent.name}
          onClose={() => setImportOpen(false)}
          onImported={(imported) => {
            setImportOpen(false);
            navigate({
              kind: 'session',
              agentId: mainAgent.id,
              sessionId: imported.sessionId,
            });
          }}
        />
      )}
      {commandsOpen && (
        <CommandMenu
          commands={commands}
          close={() => setCommandsOpen(false)}
          searchMessages={searchMessages}
          onOpenMessage={onOpenMessage}
        />
      )}
    </>
  );
}

/** The inbox's header button: amber with a count when something waits on
 *  the owner, a quiet icon otherwise. Its panel opens under it. */
function InboxButton({
  inbox,
}: {
  inbox: { count: number; render: (close: () => void) => ReactNode };
}) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  // The chat's "Open Inbox" on a suggested lesson.
  useEffect(() => onOpenInbox(() => setOpen(true)), []);
  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      setOpen(false);
      button.current?.focus();
    };
    document.addEventListener('pointerdown', outside);
    document.addEventListener('keydown', escape);
    return () => {
      document.removeEventListener('pointerdown', outside);
      document.removeEventListener('keydown', escape);
    };
  }, [open]);
  const close = () => {
    setOpen(false);
    button.current?.focus();
  };
  const waiting = inbox.count > 0;
  return (
    <div ref={root} className="needs-you-anchor">
      <button
        ref={button}
        type="button"
        className={`studio-tool-button needs-you-button${waiting ? ' has-items' : ''}`}
        aria-label={
          waiting ? `Inbox, ${inbox.count} need you` : 'Inbox, nothing waiting'
        }
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
      >
        {waiting ? (
          <>
            <i aria-hidden />
            Needs you · {inbox.count}
          </>
        ) : (
          <InboxIcon size={15} />
        )}
      </button>
      {open && (
        <div className="needs-you-popover" role="dialog" aria-label="Inbox">
          {inbox.render(close)}
        </div>
      )}
    </div>
  );
}

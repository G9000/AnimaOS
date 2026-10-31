import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import {
  isTerminalRunStatus,
  type MemorySearchResult,
  type RunMode,
  type Session,
  type UploadedAttachment,
  type BrowserSelection,
  type SessionMessageSearchHit,
  type OutcomeDefinition,
} from '@animaos/sdk';

import { AlertIcon } from './components/icons';
import { CompanionSetup } from './components/onboarding/CompanionSetup';
import { SettingsPanel } from './components/SettingsPanel';
import {
  LearningReviewPanel,
  type LearningReviewApi,
} from './components/learning/LearningReviewPanel';
import { ConnectorsView } from './components/ConnectorsView';
import { SessionSidebar } from './components/sessions/SessionSidebar';
import { ProjectHome } from './components/projects/ProjectHome';
import { ProjectsSidebar } from './components/projects/ProjectsSidebar';
import { MoveChatDialog, canMoveProjectChat } from './components/projects/MoveChatDialog';
import { SessionView } from './components/sessions/SessionView';
import { TelegramSettings } from './components/TelegramSettings';
import { DiscordSettings } from './components/DiscordSettings';
import { WorkspaceShell, availablePage } from './components/WorkspaceShell';
import { useConversationLifecycle } from './hooks/useConversationLifecycle';
import { useDaemonCompatibility } from './hooks/useDaemonCompatibility';
import { useAgentIntegrations } from './hooks/useAgentIntegrations';
import { useDiscordIntegrations } from './hooks/useDiscordIntegrations';
import { useConnectorLifetime } from './hooks/useConnectorLifetime';
import { useCompanionSessions } from './hooks/useCompanionSessions';
import { useDaemonBootstrap } from './hooks/useDaemonBootstrap';
import { useLiveSession } from './hooks/useLiveSession';
import { useMainChat } from './hooks/useMainChat';
import { useSessionCommands } from './hooks/useSessionCommands';
import { useChatModelPreferences } from './hooks/useChatModelPreferences';
import { parseChatModelCommand } from './lib/chat-model-command';
import { ShareConversationDialog } from './components/sessions/ShareConversationDialog';
import { useSessionPending } from './hooks/useSessionPending';
import { useSessionSends } from './hooks/useSessionSends';
import { useSkillCommands } from './hooks/useSkillCommands';
import { useAutomations } from './hooks/useAutomations';
import { useTranscriptActions } from './hooks/useTranscriptActions';
import { ApprovalsPage } from './pages/ApprovalsPage';
import { AutomationsPage } from './pages/AutomationsPage';
import { MemoryPage } from './pages/MemoryPage';
import { SkillsPage } from './pages/SkillsPage';
import { HealthPage } from './pages/HealthPage';
import { LogsPage } from './pages/LogsPage';
import { UsagePage } from './pages/UsagePage';
import { RuntimePage } from './pages/RuntimePage';
import {
  SESSION_MESSAGES_LIVE_POLL_MS,
  SESSION_MESSAGES_POLL_MS,
  useSessionMessages,
} from './hooks/useSessionMessages';
import { clearCheckins } from './lib/checkins';
import {
  daemon,
  toAgentDetail,
  toChatMessage,
  type AgentUpdateInput,
  type DaemonSnapshot,
} from './lib/daemon-api';
import {
  readActiveAgent,
  saveActiveAgent,
  selectActiveAgent,
} from './lib/active-agent';
import { decideApproval, pendingApprovalCount } from './lib/approvals';
import { pendingApprovals } from './lib/session-events';
import { useNeedsYou } from './hooks/useNeedsYou';
import { NeedsYouPanel } from './components/inbox/NeedsYouPanel';
import { WeeklyReport } from './components/report/WeeklyReport';
import { checkinScheduleId } from './lib/automations';
import { findAutomationMessage } from './lib/automation-cards';
import { shouldShowSession } from './lib/session-groups';
import type { ChatAttention, AutomationToast } from './lib/automation-toasts';
import { useApprovalNotifications } from './hooks/useApprovalNotifications';
import { useAutomationToasts } from './hooks/useAutomationToasts';
import { AutomationToasts } from './components/sessions/AutomationToasts';
import { useHashRoute, type HashRoute } from './lib/hash-route';
import { exportFileName, sessionKey } from './lib/session-groups';
import {
  loadDraft,
  storeDraft,
  loadLiteralDraft,
  storeLiteralDraft,
} from './lib/drafts';
import { connectorStatusLabel, safeIntegrationError } from './lib/telegram';
import {
  discordCanReply,
  discordHasRoom,
  discordStatusLabel,
  safeDiscordError,
} from './lib/discord';
import { useAttachmentSession } from './hooks/useAttachmentSession';
import { useAttachments } from './hooks/useAttachments';
import { recoveredDraft } from './lib/attachments';
import {
  validPreviewEditContext,
  type ChatReasoningEffort,
  type PreviewEditContext,
} from '@animaos/sdk';
import type { SendAttachments } from './hooks/useSessionSends';

interface AgentOperation {
  generation: number;
  lifecycleGeneration: number;
  targetAgentId: string;
}

interface FailedDraft {
  requestId: string;
  text: string;
  /** A failed message keeps its key, so resending it unchanged is joined, not doubled. */
  idempotencyKey?: string;
  attachmentIds?: string[];
  attachments?: UploadedAttachment[];
  skill?: string;
  documentEditContext?: PreviewEditContext;
  outcome?: OutcomeDefinition;
  agentId?: string;
  sessionId?: string;
}

type ChatState = {
  draft: string;
  failedDrafts: FailedDraft[];
  /** A new chat is creating its session; its first message waits. */
  sending: boolean;
  error: string | null;
  notice?: string | null;
  literalDraft?: boolean;
  /** A restored message; sent again unchanged, it reuses its key. */
  resend: {
    text: string;
    idempotencyKey: string;
    attachmentIds?: string[];
    skill?: string;
    documentEditContext?: PreviewEditContext;
    outcome?: OutcomeDefinition;
  } | null;
  /** The last Telegram reply's run; `queued` once it completed while the
   *  connector had an approved chat, which queues its reply for delivery. */
  delivery: { runId: string; queued: boolean } | null;
};

const EMPTY_CHAT: ChatState = {
  draft: '',
  failedDrafts: [],
  sending: false,
  error: null,
  resend: null,
  delivery: null,
};
const HOME_CONVERSATION = 'home';

const learningApi: LearningReviewApi = {
  policy: (agentId, options) => daemon.learningPolicy(agentId, options),
  setPolicy: (agentId, input) => daemon.setLearningPolicy(agentId, input),
  listReviews: (agentId, options) =>
    daemon.listLearningReviews(agentId, options),
  getReview: (agentId, reviewId, options) =>
    daemon.getLearningReview(agentId, reviewId, options),
  cancelReview: (agentId, reviewId) =>
    daemon.cancelLearningReview(agentId, reviewId),
  decideProposal: (agentId, reviewId, proposalId, input) =>
    daemon.decideLearningProposal(agentId, reviewId, proposalId, input),
  listSkills: (options) => daemon.listSkills(options),
  skill: (slug, options) => daemon.skill(slug, options),
  createSkillDraft: (agentId, reviewId, proposalId, input) =>
    daemon.createLearningSkillDraft(agentId, reviewId, proposalId, input),
};

function httpStatus(error: unknown): unknown {
  return typeof error === 'object' && error !== null && 'status' in error
    ? error.status
    : undefined;
}

/** Chat state is kept per agent and conversation (`home` or `session:<id>`). */
function chatKey(agentId: string, conversation: string): string {
  return `${agentId}\u0000${conversation}`;
}

function sessionConversation(sessionId: string): string {
  return `session:${sessionId}`;
}

function chatState(chats: Record<string, ChatState>, key: string): ChatState {
  return (
    chats[key] ?? {
      ...EMPTY_CHAT,
      draft: loadDraft(key),
      literalDraft: loadLiteralDraft(key),
    }
  );
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function saveTextFile(name: string, text: string) {
  const url = URL.createObjectURL(new Blob([text], { type: 'text/markdown' }));
  const link = document.createElement('a');
  link.href = url;
  link.download = name;
  link.click();
  URL.revokeObjectURL(url);
}

function ConnectingState() {
  return (
    <main
      className="relative z-[1] flex min-h-0 flex-1 items-center justify-center"
      aria-live="polite"
      aria-busy="true"
    >
      <div className="flex flex-col items-center gap-3 text-center">
        <div className="flex items-center gap-1.5" aria-hidden>
          {[0, 1, 2].map((index) => (
            <span
              key={index}
              className="typing-dot h-2 w-2 rounded-full bg-ink-3"
              style={{ animationDelay: `${index * 150}ms` }}
            />
          ))}
        </div>
        <p className="font-display text-sm font-medium text-ink">
          Connecting to anima-daemon…
        </p>
        <p className="font-mono text-[11px] text-ink-3">
          Checking daemon availability
        </p>
      </div>
    </main>
  );
}

function OfflineRetry({ retry }: { retry: () => Promise<void> }) {
  return (
    <main className="relative z-[1] flex min-h-0 flex-1 items-center justify-center px-5">
      <section
        role="alert"
        className="glass-strong w-full max-w-lg rounded-3xl p-7 text-center sm:p-9"
      >
        <div className="mx-auto flex h-12 w-12 items-center justify-center rounded-full bg-danger/10 text-danger">
          <AlertIcon size={20} />
        </div>
        <h1 className="mt-4 font-display text-2xl font-semibold tracking-tight text-ink">
          Offline
        </h1>
        <p className="mx-auto mt-2 max-w-sm text-sm leading-relaxed text-ink-2">
          The workspace cannot reach anima-daemon yet. Start the Rust host, then
          retry this connection.
        </p>
        <code className="mt-4 inline-block rounded-xl border border-line bg-abyss/60 px-3 py-2 font-mono text-xs text-mint">
          bun dev --host rust
        </code>
        <div className="mt-5">
          <button
            type="button"
            autoFocus
            onClick={() => void retry()}
            className="rounded-xl bg-accent px-4 py-2 text-sm font-semibold text-accent-fg shadow-lg shadow-accent/20 transition hover:bg-accent/90"
          >
            Retry connection
          </button>
        </div>
      </section>
    </main>
  );
}

export function ViewHarness() {
  // Once the companion's stream is open, polls slow down and events drive
  // refreshes (spec §15.5).
  const [streamOpen, setStreamOpen] = useState(false);
  const {
    connection,
    loaded,
    agents: agentSnapshots,
    providers,
    providersError,
    workspace,
    refreshAgents,
    refreshSummaries,
    retryProviders,
    refreshWorkspace,
    acceptAgentSnapshot,
    acceptAgentTools,
    removeAgentSnapshot,
  } = useDaemonBootstrap({ live: streamOpen });
  const agents = useMemo(
    () => agentSnapshots.map((snapshot) => toAgentDetail(snapshot)),
    [agentSnapshots],
  );
  // The owner can talk with several companions and switch between them;
  // helpers are implementation details, never a top-level persona.
  const [preferredAgentId, setPreferredAgentId] = useState(readActiveAgent);
  const mainAgent = selectActiveAgent(agents, preferredAgentId);
  const selectAgent = useCallback((id: string) => {
    saveActiveAgent(id);
    setPreferredAgentId(id);
  }, []);
  const agent = mainAgent;
  const agentId = agent?.id ?? null;
  const availableAgentIdsRef = useRef(new Set<string>());
  availableAgentIdsRef.current = new Set(agents.map((item) => item.id));
  const [route, navigate] = useHashRoute();
  const routeScope = useRef({ key: JSON.stringify(route), generation: 0 });
  const routeKey = JSON.stringify(route);
  if (routeScope.current.key !== routeKey)
    routeScope.current = {
      key: routeKey,
      generation: routeScope.current.generation + 1,
    };
  const routeRef = useRef(route);
  routeRef.current = route;
  // A page hides the conversation but keeps it: the last chat or session stays loaded.
  const lastConversationRef = useRef<HashRoute>({ kind: 'home' });
  if (route.kind === 'home' || route.kind === 'session') lastConversationRef.current = route;
  const conversationRoute = lastConversationRef.current;

  const [sessionQuery, setSessionQuery] = useState('');
  const [showArchived, setShowArchived] = useState(false);
  const [sessionActionError, setSessionActionError] = useState<string | null>(
    null,
  );
  const sessions = useCompanionSessions(
    agentId,
    { archived: showArchived, query: sessionQuery },
    { live: streamOpen },
  );
  // A daemon with sessions but no runs route (M2) answers a send for a
  // session that exists with 404 (spec §13.4).
  const [runsRouteMissing, setRunsRouteMissing] = useState(false);
  const listedSessionsRef = useRef(sessions.sessions);
  listedSessionsRef.current = sessions.sessions;
  const routeSessionId =
    conversationRoute.kind === 'session' ? conversationRoute.sessionId : null;
  const routeMessageId =
    conversationRoute.kind === 'session'
      ? (conversationRoute.messageId ?? null)
      : null;
  const [jumpRequestKey, setJumpRequestKey] = useState(0);
  const searchMessages = useCallback(
    (query: string, signal: AbortSignal) => {
      if (!agentId || connection !== 'online')
        return Promise.reject(new Error('Reconnect to search saved messages.'));
      return daemon.searchSessionMessages(agentId, {
        q: query,
        limit: 20,
        signal,
      });
    },
    [agentId, connection],
  );
  const searchMemories = useCallback(
    (query: string, signal: AbortSignal): Promise<MemorySearchResult[]> => {
      if (signal.aborted)
        return Promise.reject(
          signal.reason ?? new DOMException('Search canceled', 'AbortError'),
        );
      if (!agentId || connection !== 'online')
        return Promise.reject(new Error('Reconnect to search memory.'));
      return daemon.searchMemories(query, agentId, 8);
    },
    [agentId, connection],
  );
  const openMessage = (hit: SessionMessageSearchHit) => {
    setJumpRequestKey((value) => value + 1);
    navigate({
      kind: 'session',
      sessionId: hit.sessionId,
      messageId: hit.messageId,
    });
  };
  const returnToLatest = () => {
    if (conversationRoute.kind === 'session')
      navigate({ ...conversationRoute, messageId: undefined });
  };
  // Another agent's session (a helper's) names its agent in the route.
  const routeAgentId =
    conversationRoute.kind === 'session'
      ? (conversationRoute.agentId ?? agentId)
      : null;
  // A helper spawned since the last agents read would show the companion's
  // name until the next poll: a route naming an unknown agent reads the
  // summaries once for it (S3b-H).
  const summaryReadsRef = useRef(new Set<string>());
  const routeAgentKnown =
    routeAgentId === null || agents.some((item) => item.id === routeAgentId);
  useEffect(() => {
    if (!loaded || routeAgentKnown || !routeAgentId) return;
    if (summaryReadsRef.current.has(routeAgentId)) return;
    summaryReadsRef.current.add(routeAgentId);
    void refreshSummaries();
  }, [loaded, routeAgentId, routeAgentKnown, refreshSummaries]);
  const listedSession =
    routeSessionId && routeAgentId
      ? (sessions.sessions.find(
          (item) => item.id === routeSessionId && item.agentId === routeAgentId,
        ) ?? null)
      : null;
  const sessionListed = listedSession !== null;
  // A session outside the loaded list (archived, older, or filtered out by a
  // search) keeps its last known record and is read again on its own.
  const [knownSession, setKnownSession] = useState<Session | null>(null);
  const [sessionReadError, setSessionReadError] = useState<string | null>(null);
  // Bumped when the stream says the open session changed (S3b-G).
  const [sessionRereads, setSessionRereads] = useState(0);
  const [movingProjectChat, setMovingProjectChat] = useState<Session | null>(null);
  const closeProjectMove = useCallback(() => setMovingProjectChat(null), []);
  useEffect(() => {
    if (listedSession) setKnownSession(listedSession);
  }, [listedSession]);
  useEffect(() => {
    setSessionReadError(null);
    if (!routeSessionId || sessionListed || !routeAgentId) return;
    let active = true;
    let timer: number | undefined;
    // A failed read is retried on the messages' cadence while the route
    // points here; a missing session shows as deleted through its messages.
    const read = () => {
      daemon.getSession(routeAgentId, routeSessionId).then(
        (session) => {
          if (!active) return;
          setKnownSession(session);
          setSessionReadError(null);
        },
        (caught) => {
          if (!active || httpStatus(caught) === 404) return;
          setSessionReadError(errorMessage(caught));
          timer = window.setTimeout(read, SESSION_MESSAGES_POLL_MS);
        },
      );
    };
    read();
    return () => {
      active = false;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [routeAgentId, routeSessionId, sessionListed, sessionRereads]);
  const activeSession =
    listedSession ??
    (knownSession &&
    knownSession.id === routeSessionId &&
    knownSession.agentId === routeAgentId
      ? knownSession
      : null);
  // A helper's session shows the helper, not the companion.
  const sessionAgent =
    activeSession && agent && activeSession.agentId !== agent.id
      ? (agents.find((item) => item.id === activeSession.agentId) ?? agent)
      : agent;
  // The composer waits for the record: a send needs the session's kind.
  const sessionLoading = routeSessionId !== null && activeSession === null;
  const clearBoundary = useRef<{ key: string; generation: number } | null>(
    null,
  );
  const clearGeneration = activeSession?.clearGeneration ?? 0;
  useEffect(() => {
    if (!activeSession || !routeSessionId || !routeAgentId) {
      clearBoundary.current = null;
      return;
    }
    const key = `${routeAgentId}\u0000${routeSessionId}`;
    const previous = clearBoundary.current;
    clearBoundary.current = { key, generation: clearGeneration };
    if (
      previous?.key === key &&
      clearGeneration > previous.generation &&
      routeMessageId &&
      conversationRoute.kind === 'session'
    )
      navigate(
        { ...conversationRoute, messageId: undefined },
        { replace: true },
      );
  }, [
    activeSession,
    routeSessionId,
    routeAgentId,
    routeMessageId,
    clearGeneration,
    conversationRoute,
    navigate,
  ]);
  const [messagesRefresh, setMessagesRefresh] = useState(0);
  const history = useSessionMessages(
    routeSessionId ? routeAgentId : null,
    routeSessionId,
    messagesRefresh,
    streamOpen ? SESSION_MESSAGES_LIVE_POLL_MS : SESSION_MESSAGES_POLL_MS,
    {
      anchorMessageId: routeMessageId,
      jumpRequestKey,
      resetKey: clearGeneration,
    },
  );
  const chatMessages = useMemo(
    () => history.messages.map(toChatMessage),
    [history.messages],
  );
  // One identity for the view's life, so a render of the page (a streamed
  // delta, a keystroke) keeps the message list's props.
  const loadOlderRef = useRef(history.loadOlder);
  loadOlderRef.current = history.loadOlder;
  const loadOlder = useCallback(() => void loadOlderRef.current(), []);

  const conversation = routeSessionId
    ? sessionConversation(routeSessionId)
    : HOME_CONVERSATION;
  const activeChatKey = agentId ? chatKey(agentId, conversation) : null;
  const [browserChoice, setBrowserChoice] = useState<{
    key: string;
    selection: BrowserSelection;
  } | null>(null);
  useEffect(() => {
    setBrowserChoice(null);
  }, [activeChatKey]);
  const [chats, setChats] = useState<Record<string, ChatState>>({});
  const chat = activeChatKey ? chatState(chats, activeChatKey) : EMPTY_CHAT;
  const { draft, failedDrafts, sending, error: workspaceError } = chat;
  const failedDraft = failedDrafts[0]?.text ?? null;
  const storedDraftsRef = useRef(new Map<string, string>());
  useEffect(() => {
    for (const [key, value] of Object.entries(chats)) {
      const state = `${value.literalDraft ? '1' : '0'}\u0000${value.draft}`;
      if (storedDraftsRef.current.get(key) === state) continue;
      storedDraftsRef.current.set(key, state);
      storeDraft(key, value.draft);
      storeLiteralDraft(key, Boolean(value.literalDraft && value.draft));
    }
  }, [chats]);
  /** A deleted session's chat state and saved draft go with it. */
  const forgetChat = (key: string) => {
    storedDraftsRef.current.delete(key);
    storeDraft(key, '');
    setChats((current) => {
      if (!(key in current)) return current;
      const rest = { ...current };
      delete rest[key];
      return rest;
    });
  };
  const updateChat = useCallback(
    (
      key: string,
      patch: Partial<ChatState> | ((value: ChatState) => Partial<ChatState>),
    ) => {
      setChats((current) => {
        const value = chatState(current, key);
        return {
          ...current,
          [key]: {
            ...value,
            ...(typeof patch === 'function' ? patch(value) : patch),
          },
        };
      });
    },
    [],
  );
  const setDraft = useCallback(
    (value: string | ((current: string) => string)) => {
      if (activeChatKey)
        updateChat(activeChatKey, (current) => {
          const draft =
            typeof value === 'function' ? value(current.draft) : value;
          return {
            draft,
            ...(!draft && (current.resend?.documentEditContext || current.resend?.outcome) ? { resend: null } : {}),
            ...(!draft && current.literalDraft ? { literalDraft: false } : {}),
          };
        });
    },
    [activeChatKey, updateChat],
  );
  const clearDocumentEditContext = useCallback(() => {
    if (activeChatKey) updateChat(activeChatKey, { resend: null });
  }, [activeChatKey, updateChat]);
  const setFailedDrafts = (
    value: (current: ChatState['failedDrafts']) => ChatState['failedDrafts'],
  ) => {
    if (activeChatKey)
      updateChat(activeChatKey, (current) => ({
        failedDrafts: value(current.failedDrafts),
      }));
  };
  const setWorkspaceError = (error: string | null) => {
    if (activeChatKey) updateChat(activeChatKey, { error });
  };
  /** Conversations whose new chat is still creating its session. */
  const pendingSendsRef = useRef(new Set<string>());
  // Messages go to the runs route (spec §4.2): the daemon queues them, so
  // the composer stays usable while the companion works.
  const sends = useSessionSends({
    // A send still unaccepted when the page last closed (S3b-A): offered
    // back in that chat's recovery panel, its key reused if it is resent.
    onRestore: (item) =>
      updateChat(item.conversation, (current) => ({
        failedDrafts: [...current.failedDrafts, recoveredDraft(item)],
      })),
    onAccepted: (item, result) => {
      if (!availableAgentIdsRef.current.has(item.agentId)) return;
      if (item.telegram && !result.steer)
        updateChat(item.conversation, {
          delivery: { runId: result.run.id, queued: false },
        });
      // Its row shows at once, not only once the stream or ledger has it.
      if (!result.steer) live.seedRun(result.run);
      // A burst of sends settles into one read of each kind (S3b-F).
      live.refreshAllSoon();
    },
    onFailed: (item, caught) => {
      if (!availableAgentIdsRef.current.has(item.agentId)) return;
      const recover = () =>
        updateChat(item.conversation, (current) => ({
          failedDrafts: [...current.failedDrafts, recoveredDraft(item)],
          error: item.telegram
            ? safeIntegrationError(caught)
            : item.discord
              ? safeDiscordError(caught)
              : errorMessage(caught),
        }));
      if (httpStatus(caught) !== 404) {
        recover();
        return;
      }
      // The runs route answers 404 for a session that is gone, and so does
      // a daemon without the route: the session itself tells them apart.
      daemon.getSession(item.agentId, item.sessionId).then(() => {
        if (!availableAgentIdsRef.current.has(item.agentId)) return;
        // It never reached a daemon that could take it: nothing goes to
        // recovery, and the text waits in the composer for the update.
        setRunsRouteMissing(true);
        updateChat(item.conversation, (current) => ({
          draft: current.draft.trim()
            ? `${current.draft}\n\n${item.text}`
            : item.text,
        }));
      }, recover);
    },
  });
  // The last Telegram reply's run, until it is known how it ended.
  const awaitedDelivery = chat.delivery?.queued
    ? null
    : (chat.delivery?.runId ?? null);
  const watchedRunIds = useMemo(
    () => (awaitedDelivery ? [awaitedDelivery] : []),
    [awaitedDelivery],
  );
  const openRoute =
    routeAgentId && routeSessionId
      ? { agentId: routeAgentId, sessionId: routeSessionId }
      : null;
  const refreshSessions = sessions.refresh;
  // The companion's one event stream (spec §6) and the open session's runs.
  const live = useLiveSession({
    agentId,
    session: openRoute,
    replyName: sessionAgent?.name ?? 'Your companion',
    listedActiveRuns: listedSession?.activeRuns ?? null,
    messages: history.messages,
    watchedRunIds,
    refreshSessions: () => void refreshSessions(),
    refreshMessages: () => setMessagesRefresh((value) => value + 1),
    // A listed session is read again with the sidebar.
    refreshSession: () => setSessionRereads((value) => value + 1),
  });
  const slashCommands = useSkillCommands({
    version: live.state.skillsVersion,
    epoch: live.state.epoch,
    enabled: connection === 'online',
  });
  const automations = useAutomations({
    agentId,
    version: live.state.automationsVersion,
    epoch: live.state.epoch,
    enabled: connection === 'online',
  });
  const [automationFocus, setAutomationFocus] = useState<string | null>(null);
  const clearAutomationFocus = useCallback(() => setAutomationFocus(null), []);
  useEffect(() => {
    setStreamOpen(live.status === 'open');
  }, [live.status]);
  // A daemon that cannot take a send asks for an update (spec §13.4): one
  // without the sessions routes (pre-M2), or one with sessions but neither
  // the event stream nor the runs route (M2).
  const compatibility = useDaemonCompatibility({
    enabled: connection === 'online',
    epoch: live.state.epoch,
  });
  const daemonTooOld =
    compatibility.statusUnsupported ||
    sessions.daemonTooOld ||
    live.status === 'unsupported' ||
    runsRouteMissing;
  const activeRun = live.activeRun;
  const openPending = useSessionPending({
    sends: sends.sends,
    settle: sends.settle,
    session: activeSession ? openRoute : null,
    messages: history.messages,
    runs: live.runs,
    ledger: live.ledger,
    appliedRead: history.appliedRead,
    readsStarted: history.readsStarted,
    refreshMessages: history.refresh,
    refreshRuns: live.refreshRuns,
    onRecover: (item) => {
      if (!availableAgentIdsRef.current.has(item.agentId)) return;
      // A new key when sent again: the steer's may still name its old run.
      updateChat(item.conversation, (current) => ({
        failedDrafts: [...current.failedDrafts, recoveredDraft(item, false)],
      }));
    },
  });
  // A listing read before the stream's newest events does not keep
  // "is thinking" up: the open stream counts the active runs.
  const { activeRunCount } = live;
  const viewedSession = useMemo(
    () =>
      activeSession && activeRunCount !== null
        ? { ...activeSession, activeRuns: activeRunCount }
        : activeSession,
    [activeSession, activeRunCount],
  );
  const [settingsSaveError, setSettingsSaveError] = useState<string | null>(
    null,
  );
  const [pendingSkillCounts, setPendingSkillCounts] = useState<
    Record<string, number>
  >({});
  const [resetError, setResetError] = useState<string | null>(null);
  const [savingSettings, setSavingSettings] = useState(false);
  const [mcpBusy, setMcpBusy] = useState(false);
  const mcpBusyRef = useRef(false);
  const mcpPermissionsUnknownRef = useRef(false);
  const connectorLifetime = useConnectorLifetime(
    agentId,
    connection === 'online',
    live.state.epoch,
  );
  useLayoutEffect(() => {
    mcpBusyRef.current = false;
    setMcpBusy(false);
  }, [connectorLifetime.token]);
  const [resetting, setResetting] = useState(false);
  const mainChat = useMainChat({
    agentId: resetting ? null : agentId,
    online: connection === 'online' && loaded,
  });
  const waitingForMain =
    conversationRoute.kind === 'home' && mainChat.session === null;
  const upsertMainSession = sessions.upsert;
  useEffect(() => {
    const session = mainChat.session;
    if (
      !session ||
      session.agentId !== agentId ||
      resetting ||
      lastConversationRef.current.kind !== 'home'
    )
      return;
    const homeKey = chatKey(session.agentId, HOME_CONVERSATION);
    const target = chatKey(session.agentId, sessionConversation(session.id));
    // Preserve a saved home draft and an existing main-chat draft on adoption.
    setChats((current) => {
      const home = chatState(current, homeKey);
      if (!home.draft && home.failedDrafts.length === 0) return current;
      const existing = chatState(current, target);
      return {
        ...current,
        [homeKey]: { ...home, draft: '', failedDrafts: [] },
        [target]: {
          ...existing,
          draft: [existing.draft, home.draft].filter(Boolean).join('\n\n'),
          failedDrafts: [...existing.failedDrafts, ...home.failedDrafts],
        },
      };
    });
    setKnownSession(session);
    upsertMainSession(session);
    const resolved: HashRoute = { kind: 'session', sessionId: session.id };
    if (availablePage(routeRef.current) !== null || routeRef.current.kind === 'project')
      lastConversationRef.current = resolved;
    else navigate(resolved, { replace: true });
  }, [
    agentId,
    mainChat.session,
    resetting,
    conversationRoute.kind,
    navigate,
    upsertMainSession,
  ]);
  // The transcript's newest message is in view: an automation card that
  // lands outside the view gets a toast.
  const mainAtBottomRef = useRef(true);
  const onMainAtBottomChange = useCallback((atBottom: boolean) => {
    mainAtBottomRef.current = atBottom;
  }, []);
  const mainSessionIdRef = useRef<string | null>(null);
  mainSessionIdRef.current = mainChat.session?.id ?? null;
  const agentIdRef = useRef(agentId);
  agentIdRef.current = agentId;
  const chatAttention = useCallback((): ChatAttention => {
    const current = routeRef.current;
    const mainId = mainSessionIdRef.current;
    const viewingMainChat =
      current.kind === 'home' ||
      (current.kind === 'session' &&
        mainId !== null &&
        current.sessionId === mainId &&
        (!current.agentId || current.agentId === agentIdRef.current));
    return {
      viewingMainChat,
      atBottom: mainAtBottomRef.current,
      documentHidden: document.hidden,
    };
  }, []);
  const automationToasts = useAutomationToasts({
    agentId,
    mainSessionId: mainChat.session?.id ?? null,
    enabled: connection === 'online',
    attention: chatAttention,
  });
  const [legacyMigrationError, setLegacyMigrationError] = useState<
    string | null
  >(null);

  const savingSettingsRef = useRef(false);
  const agentOperationGenerationRef = useRef(0);
  const agentLifecycleGenerationRef = useRef(0);
  const settingsOperationGenerationRef = useRef<number | null>(null);
  const resetInFlightRef = useRef<AgentOperation | null>(null);
  const currentAgentIdRef = useRef<string | null>(null);
  const previousSelectedMainIdRef = useRef<string | null>(null);

  const integrations = useAgentIntegrations(
    agentId,
    connection === 'online',
    live.state.epoch,
  );
  const discordIntegrations = useDiscordIntegrations(
    agentId,
    (route.kind === 'page' && route.page === 'connectors') ||
      activeSession?.kind === 'discord',
    connection === 'online',
    live.state.epoch,
  );
  const telegramConnector = integrations.connectors[0] ?? null;
  const [reportOpen, setReportOpen] = useState(false);
  const approvalsWaiting = useMemo(
    () => pendingApprovals(live.state.approvals),
    [live.state.approvals],
  );
  const approvalAgentName = useCallback(
    (id: string) => agents.find((item) => item.id === id)?.name ?? 'An agent',
    [agents],
  );
  useApprovalNotifications(approvalsWaiting, approvalAgentName);
  const needsYou = useNeedsYou({
    agentId,
    online: connection === 'online',
    approvals: approvalsWaiting,
    telegram: integrations.connectors,
    sessions: sessions.sessions,
    refreshKey: `${live.state.skillsVersion}:${live.state.epoch}`,
  });
  const activeConnector =
    activeSession?.kind === 'telegram'
      ? (integrations.connectors.find(
          (item) => item.roomId === activeSession.roomId,
        ) ?? null)
      : null;
  const activeDiscord =
    activeSession?.kind === 'discord'
      ? (discordIntegrations.connectors.find((item) =>
          discordHasRoom(item, activeSession.roomId),
        ) ?? null)
      : null;
  const discordReady =
    discordIntegrations.available &&
    !discordIntegrations.loading &&
    discordIntegrations.busy === null &&
    discordCanReply(activeDiscord);
  const connectorReplyAvailable =
    activeSession?.kind === 'discord'
      ? discordReady
      : activeSession?.kind !== 'telegram' || activeConnector !== null;
  // The connector queues a Telegram reply for delivery when its run
  // completes while it has an approved chat. How the run ended comes from
  // its lifecycle event or the ledger, which is read once its messages
  // arrive (useLiveSession's `watchedRunIds`).
  const deliveryApproved = activeConnector?.approvedChat != null;
  const deliveryRun = awaitedDelivery
    ? (live.runs.find((item) => item.run.id === awaitedDelivery)?.run ?? null)
    : null;
  useEffect(() => {
    if (
      !deliveryRun ||
      !activeChatKey ||
      !isTerminalRunStatus(deliveryRun.status)
    )
      return;
    const runId = deliveryRun.id;
    const queued = deliveryRun.status === 'completed' && deliveryApproved;
    updateChat(activeChatKey, (current) =>
      current.delivery?.runId === runId
        ? { delivery: queued ? { runId, queued } : null }
        : {},
    );
  }, [activeChatKey, deliveryRun, deliveryApproved, updateChat]);
  useLayoutEffect(() => {
    if (previousSelectedMainIdRef.current === agentId) return;

    const previousAgentId = previousSelectedMainIdRef.current;
    previousSelectedMainIdRef.current = agentId;
    agentLifecycleGenerationRef.current += 1;
    agentOperationGenerationRef.current += 1;
    currentAgentIdRef.current = agentId;
    settingsOperationGenerationRef.current = null;
    resetInFlightRef.current = null;
    savingSettingsRef.current = false;
    mcpBusyRef.current = false;
    mcpPermissionsUnknownRef.current = false;

    setLegacyMigrationError(null);
    setSessionActionError(null);
    setSettingsSaveError(null);
    setResetError(null);
    setSavingSettings(false);
    setMcpBusy(false);
    setResetting(false);
    // Another companion has other sessions: start from a new chat.
    if (previousAgentId !== null) navigate({ kind: 'home' }, { replace: true });
  }, [agentId, navigate]);

  const beginAgentOperation = useCallback(
    (targetAgentId: string): AgentOperation => ({
      generation: ++agentOperationGenerationRef.current,
      lifecycleGeneration: agentLifecycleGenerationRef.current,
      targetAgentId,
    }),
    [],
  );

  const isCurrentAgentOperation = useCallback(
    (operation: AgentOperation) =>
      operation.generation === agentOperationGenerationRef.current &&
      operation.lifecycleGeneration === agentLifecycleGenerationRef.current &&
      operation.targetAgentId === currentAgentIdRef.current,
    [],
  );

  const isCurrentResetOperation = useCallback(
    (operation: AgentOperation) =>
      resetInFlightRef.current === operation &&
      operation.lifecycleGeneration === agentLifecycleGenerationRef.current &&
      operation.targetAgentId === currentAgentIdRef.current,
    [],
  );

  const adoptAgentSnapshot = useCallback(
    (operation: AgentOperation, snapshot: DaemonSnapshot) => {
      if (
        !isCurrentAgentOperation(operation) ||
        snapshot.state.id !== operation.targetAgentId
      ) {
        return false;
      }

      acceptAgentSnapshot(snapshot);
      return true;
    },
    [acceptAgentSnapshot, isCurrentAgentOperation],
  );

  const scrollerRef = useRef<HTMLDivElement>(null);
  const scrollDown = () => {
    requestAnimationFrame(() => {
      const element = scrollerRef.current;
      if (element) element.scrollTop = element.scrollHeight;
    });
  };

  const conversationHidden = availablePage(route) !== null || route.kind === 'project';
  useConversationLifecycle({
    agentId,
    activeSession,
    conversationHidden,
    messages: history.messages,
    refreshSessions,
    setLegacyMigrationError,
  });

  // While the stream is open agents are polled slowly; a page that edits the
  // companion reads its record first so a save starts from the current one.
  const editingPage =
    route.kind === 'page' &&
    (route.page === 'profile' ||
      route.page === 'model' ||
      route.page === 'approvals')
      ? route.page
      : null;
  const refreshAgentsRef = useRef(refreshAgents);
  refreshAgentsRef.current = refreshAgents;
  useEffect(() => {
    if (editingPage && streamOpen) void refreshAgentsRef.current();
    // Only entering a page reads again, not every poll.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editingPage]);

  const changeWorkspaceAvatar = useCallback(
    async (file: File) => {
      await daemon.uploadWorkspaceAvatar(file);
      await refreshWorkspace();
    },
    [refreshWorkspace],
  );

  const saveSettings = async (patch: AgentUpdateInput): Promise<boolean> => {
    if (
      !agent ||
      mcpBusyRef.current ||
      mcpPermissionsUnknownRef.current ||
      savingSettingsRef.current ||
      resetInFlightRef.current !== null
    ) {
      return false;
    }
    const operation = beginAgentOperation(agent.id);
    settingsOperationGenerationRef.current = operation.generation;
    savingSettingsRef.current = true;
    setSavingSettings(true);
    setSettingsSaveError(null);
    setResetError(null);
    try {
      const { agent: updatedAgent } = await daemon.updateAgent(agent.id, patch);
      const adopted = adoptAgentSnapshot(operation, updatedAgent);
      if (adopted) {
        setSettingsSaveError(null);
        setSessionRereads((value) => value + 1);
      }
      return adopted;
    } catch (caught) {
      if (isCurrentAgentOperation(operation)) {
        setSettingsSaveError(errorMessage(caught));
      }
      return false;
    } finally {
      if (settingsOperationGenerationRef.current === operation.generation) {
        settingsOperationGenerationRef.current = null;
        savingSettingsRef.current = false;
        setSavingSettings(false);
      }
    }
  };

  const resetAgent = async () => {
    if (
      !agent ||
      mcpBusyRef.current ||
      mcpPermissionsUnknownRef.current ||
      savingSettingsRef.current ||
      resetInFlightRef.current !== null
    ) {
      return;
    }
    const targetAgentId = agent.id;
    const operation = beginAgentOperation(targetAgentId);
    resetInFlightRef.current = operation;
    setResetting(true);
    setResetError(null);
    setSettingsSaveError(null);
    try {
      const [personality, core, recent] = await Promise.all([
        daemon.agentPersonality(targetAgentId),
        daemon.coreMemory(targetAgentId),
        daemon.recentAgentMemories(targetAgentId, 1000),
      ]);
      if (personality.source === 'workspace_soul') {
        throw new Error(
          'This agent reads a shared workspace SOUL.md. Reset it from Files so the shared instructions are not changed for other agents.',
        );
      }
      const agentMemories = recent.memories.filter(
        (memory) => memory.scope !== 'shared',
      );
      const clearCore = [
        ...core.user.entries.map((entry) =>
          daemon.removeCoreEntry(targetAgentId, 'user', entry.id),
        ),
        ...core.notes.entries.map((entry) =>
          daemon.removeCoreEntry(targetAgentId, 'notes', entry.id),
        ),
      ];
      const clearMemory = agentMemories.map((memory) =>
        daemon.deleteMemory(memory.id, memory.content),
      );
      const removals = [...clearCore, ...clearMemory];
      for (let index = 0; index < removals.length; index += 25) {
        await Promise.all(removals.slice(index, index + 25));
      }
      const balanced = [
        '# Soul',
        `You are ${agent.name}, the owner’s personal companion. Keep a consistent identity across conversations and channels.`,
        '',
        '## How you talk',
        'Suggest a useful next step when it helps, and keep answers concise.',
        '',
        '## Boundaries',
        'Stay within granted permissions. Ask before spending money, publishing, deleting important data, or sending consequential external messages.',
      ].join('\n');
      await daemon.saveAgentPersonality(targetAgentId, {
        content: balanced,
        revision: personality.revision,
      });
      window.dispatchEvent(
        new CustomEvent('anima:personality-changed', { detail: targetAgentId }),
      );
      if (isCurrentResetOperation(operation)) setResetError(null);
    } catch (caught) {
      if (isCurrentResetOperation(operation)) {
        setResetError(errorMessage(caught));
      }
    } finally {
      if (resetInFlightRef.current === operation) {
        resetInFlightRef.current = null;
        setResetting(false);
      }
    }
  };

  const deleteAgent = async () => {
    if (
      !agent ||
      mcpBusyRef.current ||
      mcpPermissionsUnknownRef.current ||
      savingSettingsRef.current ||
      resetInFlightRef.current !== null
    ) {
      return;
    }
    const targetAgentId = agent.id;
    const operation = beginAgentOperation(targetAgentId);
    resetInFlightRef.current = operation;
    setResetting(true);
    setResetError(null);
    setSettingsSaveError(null);
    try {
      try {
        await daemon.deleteAgent(targetAgentId);
      } catch (caught) {
        if (isCurrentResetOperation(operation)) {
          setResetError(errorMessage(caught));
        }
        return;
      }

      const ownsSelectedMain = isCurrentResetOperation(operation);
      if (ownsSelectedMain) {
        agentLifecycleGenerationRef.current += 1;
        agentOperationGenerationRef.current += 1;
        currentAgentIdRef.current = null;
      }
      availableAgentIdsRef.current.delete(targetAgentId);
      removeAgentSnapshot(targetAgentId);
      sends.forgetAgent(targetAgentId);
      try {
        clearCheckins(targetAgentId);
      } catch {
        // The daemon deletion is authoritative; local cleanup is best-effort.
      }
    } finally {
      if (resetInFlightRef.current === operation) {
        resetInFlightRef.current = null;
        setResetting(false);
      }
    }
  };

  /** Hands a message to the send queue (spec §4.2): retried with its key,
   *  in order with the session's other messages. */
  const consumeBrowserChoice = (): BrowserSelection | null => {
    const captured =
      browserChoice?.key === activeChatKey ? browserChoice : null;
    if (!captured) return null;
    setBrowserChoice((current) => (current === captured ? null : current));
    return {
      policyId: captured.selection.policyId,
      ...(captured.selection.profileId === undefined
        ? {}
        : { profileId: captured.selection.profileId }),
      selectedInputIds: [...captured.selection.selectedInputIds],
    };
  };

  // Plan mode is the owner's choice for the chats they send from now on;
  // each message tells the daemon which it is.
  const [planMode, setPlanMode] = useState(false);
  const planModeRef = useRef(planMode);
  planModeRef.current = planMode;

  const queueSend = (
    target: Pick<Session, 'agentId' | 'id' | 'kind'>,
    conversation: string,
    text: string,
    idempotencyKey: string,
    mode: RunMode = 'queue',
    skill?: string,
    attachmentInput?: SendAttachments,
    documentEditContext?: PreviewEditContext,
    outcome?: OutcomeDefinition,
    capturedBrowserSelection?: BrowserSelection | null,
  ) => {
    if (
      !availableAgentIdsRef.current.has(target.agentId) ||
      resetInFlightRef.current !== null
    )
      return;
    if (documentEditContext !== undefined && (target.kind !== 'chat' || mode !== 'queue' || !validPreviewEditContext(documentEditContext) || conversation !== `${target.agentId}\u0000session:${target.id}`)) {
      updateChat(conversation, { error: 'Select the original content again in its web chat before sending this edit.' }); return;
    }
    const browserSelection =
      documentEditContext ? null : capturedBrowserSelection !== undefined
        ? capturedBrowserSelection
        : target.agentId === agentId &&
            target.kind === 'chat' &&
            mode === 'queue' &&
            (!routeSessionId || target.id === routeSessionId)
          ? consumeBrowserChoice()
          : null;
    sends.send({
      key: idempotencyKey,
      agentId: target.agentId,
      sessionId: target.id,
      conversation,
      text,
      ...(documentEditContext ? { documentEditContext } : {}),
      ...(outcome ? { outcome } : {}),
      mode,
      ...(skill ? { skill } : {}),
      ...(target.kind === 'chat' &&
      !documentEditContext &&
      planModeRef.current &&
      agent?.toolNames.includes('propose_plan')
        ? { plan: true }
        : {}),
      ...attachmentInput,
      telegram: target.kind === 'telegram',
      discord: target.kind === 'discord',
      ...(browserSelection
        ? {
            browserSelection: {
              policyId: browserSelection.policyId,
              ...(browserSelection.profileId === undefined
                ? {}
                : { profileId: browserSelection.profileId }),
              selectedInputIds: browserSelection.selectedInputIds.filter((id) =>
                attachmentInput?.attachmentIds.includes(id),
              ),
            },
          }
        : {}),
    });
  };

  const attachmentSession = useAttachmentSession({
    chatKey: activeChatKey,
    agentId,
    session: activeSession,
    createSession: mainChat.ensureSession,
  });
  const attachments = useAttachments({
    chatKey: activeChatKey,
    agentId: activeSession?.agentId ?? agentId,
    sessionId: routeSessionId,
    clearGeneration,
    ensureSession: () =>
      attachmentSession.ensureSession().then((session) => session.id),
    enabled:
      agent !== null &&
      connection === 'online' &&
      !resetting &&
      !sessionLoading &&
      !waitingForMain &&
      !daemonTooOld &&
      activeSession?.kind !== 'discord' &&
      (activeSession?.capabilities.send ?? true) &&
      connectorReplyAvailable,
  });
  const savedAttachmentIds = attachments.readyAttachmentIds;

  /** Sending and attachment uploads share the durable main-chat identity. */
  const startChat = async (
    targetId: string,
    text: string,
    skill?: string,
    attachmentInput?: SendAttachments,
    outcome?: OutcomeDefinition,
  ) => {
    const homeKey = chatKey(targetId, HOME_CONVERSATION);
    const lifecycleGeneration = agentLifecycleGenerationRef.current;
    if (pendingSendsRef.current.has(homeKey)) return;
    // Consume before session creation: a later completion owns only this snapshot.
    const capturedBrowserSelection = consumeBrowserChoice();
    pendingSendsRef.current.add(homeKey);
    updateChat(homeKey, { sending: true, error: null, draft: '' });
    let session: Session;
    try {
      session = await attachmentSession.ensureSession();
    } catch (caught) {
      pendingSendsRef.current.delete(homeKey);
      updateChat(homeKey, (current) => ({
        sending: false,
        failedDrafts: [
          ...current.failedDrafts,
          { requestId: crypto.randomUUID(), text, ...(outcome ? { outcome } : {}) },
        ],
        error: errorMessage(caught),
      }));
      return;
    }
    pendingSendsRef.current.delete(homeKey);
    attachmentSession.forget(homeKey);
    if (
      currentAgentIdRef.current !== targetId ||
      agentLifecycleGenerationRef.current !== lifecycleGeneration ||
      resetInFlightRef.current !== null
    ) {
      updateChat(homeKey, { sending: false });
      return;
    }
    const target = chatKey(targetId, sessionConversation(session.id));
    // Text typed while the chat was created moves with it.
    setChats((current) => {
      const home = chatState(current, homeKey);
      return {
        ...current,
        [homeKey]: { ...home, draft: '', sending: false },
        [target]: { ...chatState(current, target), draft: home.draft },
      };
    });
    sessions.upsert(session);
    const created: HashRoute = { kind: 'session', sessionId: session.id };
    // Follow the new chat only while it is still the conversation on screen
    // or behind a page: a session opened meanwhile keeps the owner, a page
    // that hides the chat stays open with the session behind it, and a page
    // that shows the chat moves to the session so a reload finds it.
    if (lastConversationRef.current.kind === 'home') {
      if (availablePage(routeRef.current) !== null || routeRef.current.kind === 'project')
        lastConversationRef.current = created;
      else navigate(created, { replace: true });
    }
    queueSend(
      session,
      target,
      text,
      crypto.randomUUID(),
      'queue',
      skill,
      attachmentInput,
      undefined,
      outcome,
      capturedBrowserSelection,
    );
  };

  const newChat = () =>
    navigate(
      mainChat.session
        ? { kind: 'session', sessionId: mainChat.session.id }
        : { kind: 'home' },
    );
  const renameSession = async (session: Session, title: string) => {
    try {
      await daemon.updateSession(session.agentId, session.id, { title });
      setSessionActionError(null);
      await sessions.refresh();
      return true;
    } catch (caught) {
      setSessionActionError(errorMessage(caught));
      return false;
    }
  };
  const archiveSession = async (session: Session, archived: boolean) => {
    try {
      await daemon.updateSession(session.agentId, session.id, { archived });
      setSessionActionError(null);
      await sessions.refresh();
      return true;
    } catch (caught) {
      setSessionActionError(errorMessage(caught));
      return false;
    }
  };
  const exportSession = async (session: Session) => {
    try {
      saveTextFile(
        exportFileName(session.title),
        await daemon.exportSession(session.agentId, session.id),
      );
      setSessionActionError(null);
    } catch (caught) {
      setSessionActionError(errorMessage(caught));
    }
  };
  const deleteSession = async (session: Session) => {
    try {
      await daemon.deleteSession(session.agentId, session.id);
      setSessionActionError(null);
      sessions.remove(session);
      if (agentId)
        forgetChat(chatKey(agentId, sessionConversation(session.id)));
      // Judge by the route now: a page or another session may have opened meanwhile.
      const open = lastConversationRef.current;
      if (open.kind === 'session' && open.sessionId === session.id) {
        if (routeRef.current.kind === 'page' || routeRef.current.kind === 'project')
          lastConversationRef.current = { kind: 'home' };
        else navigate({ kind: 'home' }, { replace: true });
      }
      return true;
    } catch (caught) {
      setSessionActionError(errorMessage(caught));
      return false;
    }
  };

  const chatModel = useChatModelPreferences(
    activeSession,
    daemon.chatModels,
    sessionRereads,
  );
  // Second opinions live on each reply; /review asks about the latest one
  // and bumps this so the reply reads its review.
  const [reviewRefresh, setReviewRefresh] = useState(0);
  const [sharing, setSharing] = useState<Session | null>(null);
  const reviewsChat = (session: Session | null): session is Session =>
    session?.kind === 'chat' &&
    ['web', 'api'].includes(session.origin) &&
    !session.parentRunId &&
    daemon.chatModels !== undefined;
  const startReview = (focus?: string) => {
    const session = activeSession;
    if (!reviewsChat(session)) return;
    void daemon.chatModels
      .review(session.agentId, session.id, {
        requestId: crypto.randomUUID(),
        clearGeneration: session.clearGeneration ?? 0,
        ...(focus ? { focus } : {}),
      })
      .then(() => setReviewRefresh((value) => value + 1))
      .catch((caught) => setWorkspaceError(errorMessage(caught)));
  };
  /** `/model <provider> <model> [effort]` switches this chat; without an
   *  argument, every model and the agent's default are in Settings. */
  const chooseChatModel = (argument = '') => {
    if (!activeSession || !argument.trim()) {
      navigate({ kind: 'page', page: 'model' });
      return;
    }
    try {
      const preference = parseChatModelCommand(argument);
      if (preference) void chatModel.update(preference);
    } catch (caught) {
      setWorkspaceError(errorMessage(caught));
    }
  };

  const commands = useSessionCommands({
    companionId: agentId,
    canSend: () =>
      agent !== null &&
      connection === 'online' &&
      resetInFlightRef.current === null &&
      !daemonTooOld,
    routeSessionId,
    navigationScope: `${routeScope.current.generation}:${routeKey}`,
    session: activeSession,
    chatKey: activeChatKey,
    draft,
    literalDraft: chat.literalDraft,
    resend: chat.resend,
    attachments: attachments.readyAttachments,
    attachmentIds: savedAttachmentIds,
    attachmentsBlocked: attachments.items.some(
      (item) => item.status !== 'ready',
    ),
    clearAttachments: attachments.clear,
    telegramReady: activeConnector !== null,
    discordReady,
    activeRun,
    updateChat,
    startChat: (text, skill, attachmentInput, outcome) => {
      if (agent) void startChat(agent.id, text, skill, attachmentInput, outcome);
    },
    queueSend,
    slashCommands,
    refreshRuns: live.refreshRuns,
    refreshMessages: () => setMessagesRefresh((value) => value + 1),
    setError: setWorkspaceError,
    navigate,
    listedSessions: sessions.sessions,
    upsertSession: sessions.upsert,
    setKnownSession,
    newChat,
    showCommands: () => setDraft('/'),
    search: setSessionQuery,
    chooseModel: chooseChatModel,
    openTools: () => navigate({ kind: 'page', page: 'capabilities' }),
    review: startReview,
    share: () => {
      if (reviewsChat(activeSession)) setSharing(activeSession);
    },
    rename: renameSession,
    archive: archiveSession,
    exportSession,
  });
  const { stopRun } = commands;
  const openSession = (session: Session) =>
    commands.openTarget({ agentId: session.agentId, sessionId: session.id });
  /** A session, at one of its messages when given (a Discord note's "Open
   *  thread"). */
  const openSessionAt = (sessionId: string, messageId?: string) => {
    if (messageId) setJumpRequestKey((value) => value + 1);
    navigate({
      kind: 'session',
      sessionId,
      ...(messageId ? { messageId } : {}),
    });
  };
  /** "Show in chat": the main chat, at the automation's card when that
   *  message is loaded, else just open it. */
  const showAutomationInChat = (target: {
    automationId: string;
    runId?: string | null;
  }) => {
    const mainId = mainChat.session?.id ?? null;
    if (!mainId) {
      navigate({ kind: 'home' });
      return;
    }
    const loaded =
      conversationRoute.kind === 'session' &&
      conversationRoute.sessionId === mainId
        ? history.messages
        : [];
    openSessionAt(mainId, findAutomationMessage(loaded, target) ?? undefined);
  };
  const openToast = (toast: AutomationToast) => {
    automationToasts.dismiss(toast.id);
    const { meta } = toast;
    if (meta.kind === 'automationNote' && meta.destinationSessionId)
      openSessionAt(meta.destinationSessionId, meta.destinationMessageId);
    else newChat();
  };
  const transcriptActions = useTranscriptActions({
    session: activeSession,
    // A Telegram session's messages need its connector.
    resendable: connectorReplyAvailable,
    liveRuns: live.state.runs,
    sessions: sessions.sessions,
    stopRun: (run) => void stopRun(run),
    // Its text goes back to the recovery panel with its key, so a resend
    // joins a request that did reach the daemon (S3b-I).
    cancelPending: (key) => {
      const item = sends.cancel(key);
      if (!item) return;
      updateChat(item.conversation, (current) => ({
        failedDrafts: [...current.failedDrafts, recoveredDraft(item)],
      }));
    },
    sendAgain: commands.sendAgain,
    compact: (session) => void commands.compactSession(session),
    compacting: commands.compacting,
    openSession: commands.openTarget,
    companionId: agentId,
    automations: automations.automations,
    undoAutomation: automations.undo,
    automationsLoaded: automations.loaded,
    online: connection === 'online',
    completeAutomation: automations.complete,
    snoozeAutomation: automations.snooze,
    openSessionAt,
  });
  const reportPendingSkillCount = useCallback(
    (count: number) => {
      const currentAgentId = agent?.id;
      if (!currentAgentId) return;
      setPendingSkillCounts((current) =>
        current[currentAgentId] === count
          ? current
          : { ...current, [currentAgentId]: count },
      );
    },
    [agent?.id],
  );

  if (connection === 'unknown' || (connection === 'online' && !loaded)) {
    return <ConnectingState />;
  }

  if (!agent && connection === 'offline') {
    return <OfflineRetry retry={refreshAgents} />;
  }

  const onboardingLifecycleGeneration = agentLifecycleGenerationRef.current;
  if (!agent) {
    return (
      <CompanionSetup
        providers={providers}
        providersError={providersError}
        retryProviders={retryProviders}
        onCreated={(snapshot) => {
          if (
            currentAgentIdRef.current !== null ||
            agentLifecycleGenerationRef.current !==
              onboardingLifecycleGeneration
          ) {
            return;
          }
          agentOperationGenerationRef.current += 1;
          agentLifecycleGenerationRef.current += 1;
          acceptAgentSnapshot(snapshot);
          void refreshWorkspace();
          scrollDown();
        }}
      />
    );
  }

  // The companion's own settings, split by what the owner is deciding: each
  // page shows its part and keeps the save and reset guards.
  // A save or reset holds the window on its page; an external tool grant
  // keeps running when the owner leaves Connections.
  const settingsBusy = savingSettings || resetting;
  const settingsPage = (
    sections: Parameters<typeof SettingsPanel>[0]['sections'],
  ) => (
    <SettingsPanel
      key={sections?.join()}
      refreshProviders={retryProviders}
      agent={agent}
      agents={agents}
      providers={providers}
      saving={savingSettings || mcpBusy}
      resetting={resetting}
      saveError={settingsSaveError}
      resetError={resetError}
      saveSettings={saveSettings}
      resetAgent={resetAgent}
      deleteAgent={deleteAgent}
      isMainAgent={agent.workspaceRole !== 'helper'}
      sections={sections}
    />
  );

  const sidebar = (
    <>
    <ProjectsSidebar agentId={agent.id} navigate={navigate} />
    <SessionSidebar
      agents={agents}
      // The main chat is where you are; History lists everything else.
      sessions={sessions.sessions.filter(
        (session) => session.mainChat !== true && shouldShowSession(session),
      )}
      activeKey={activeSession ? sessionKey(activeSession) : null}
      query={sessionQuery}
      onQueryChange={setSessionQuery}
      showArchived={showArchived}
      onShowArchivedChange={setShowArchived}
      error={sessionActionError ?? (daemonTooOld ? null : sessions.error)}
      hasMore={sessions.hasMore}
      loadingMore={sessions.loadingMore}
      onLoadMore={() => void sessions.loadMore()}
      onOpen={openSession}
      onRename={renameSession}
      onArchive={archiveSession}
      onExport={exportSession}
      onDelete={deleteSession}
      onMoveToProject={setMovingProjectChat}
    />
    </>
  );

  // A helper session's user turns came from the agent that delegated or
  // sent them (M2 final review).
  const delegatedBy =
    activeSession?.kind === 'helper'
      ? (agents.find((item) => item.id === activeSession.parentAgentId)?.name ??
        agent.name)
      : null;

  const checkinId =
    viewedSession?.kind === 'checkin'
      ? checkinScheduleId(viewedSession.id)
      : null;
  const checkinAutomation = checkinId
    ? (automations.automations.find((item) => item.id === checkinId) ?? null)
    : null;

  const sessionView = (
    <SessionView
      agent={sessionAgent ?? agent}
      browserTask={{
        selection:
          browserChoice?.key === activeChatKey ? browserChoice.selection : null,
        attachments: attachments.readyAttachments,
        onSelection: (selection) =>
          setBrowserChoice(
            selection && activeChatKey
              ? { key: activeChatKey, selection }
              : null,
          ),
      }}
      session={viewedSession}
      onMoveToProject={viewedSession && canMoveProjectChat(viewedSession) ? () => setMovingProjectChat(viewedSession) : undefined}
      onOpenProject={(projectId) => navigate({ kind: 'project', projectId })}
      secondOpinions={
        reviewsChat(viewedSession)
          ? { api: daemon.chatModels, refreshKey: reviewRefresh }
          : undefined
      }
      historyAnchor={
        routeMessageId
          ? {
              messageId: routeMessageId,
              loading: history.loadingAnchor,
              error: history.error,
              onReturnToLatest: returnToLatest,
            }
          : undefined
      }
      jumpRequestKey={jumpRequestKey}
      onAtBottomChange={onMainAtBottomChange}
      contextRefreshKey={`${messagesRefresh}:${sessionRereads}:${activeSession?.lastActivityAtMs}:${activeSession?.messageCount}`}
      skillDrafts={{
        version: live.state.skillsVersion,
        epoch: live.state.epoch,
        enabled: connection === 'online',
        onReview: () => navigate({ kind: 'page', page: 'skills' }),
      }}
      messages={routeSessionId ? chatMessages : []}
      knownAttachments={sends.knownRunAttachments}
      pending={routeMessageId ? undefined : openPending}
      runs={routeMessageId ? undefined : live.runs}
      actions={transcriptActions}
      automation={checkinAutomation}
      onEditAutomation={(automation) => {
        setAutomationFocus(automation.id);
        navigate({ kind: 'page', page: 'automations' });
      }}
      delegatedBy={delegatedBy}
      announcement={live.announcement}
      hasOlder={history.hasOlder}
      loadingOlder={history.loadingOlder}
      loadingHistory={
        routeSessionId !== null &&
        history.appliedRead === 0 &&
        !history.missing &&
        !history.error
      }
      onLoadOlder={loadOlder}
      missing={routeSessionId !== null && history.missing && !daemonTooOld}
      telegramAvailable={activeConnector !== null}
      discordAvailable={discordReady}
      scrollerRef={scrollerRef}
      onSuggestion={setDraft}
      composer={{
        draft,
        literalDraft: chat.literalDraft,
        conversationKey: activeChatKey,
        voiceDisabled: conversationHidden,
        nativeVoice:
          agentId && (!activeSession || activeSession.kind === 'chat')
            ? {
                agentId: activeSession?.agentId ?? agentId,
                ...(routeSessionId ? { sessionId: routeSessionId } : {}),
                ensureSession: () =>
                  attachmentSession
                    .ensureSession()
                    .then((session) => session.id),
              }
            : undefined,
        attachments: {
          items: attachments.items,
          onAdd: attachments.add,
          onRemove: attachments.remove,
          onUseSaved: (attachment) => attachments.restore([attachment]),
          error: attachments.error,
        },
        savedAttachmentCount: savedAttachmentIds.length,
        mode: {
          plan: planMode,
          canPlan: Boolean(agent?.toolNames.includes('propose_plan')),
          onChange: setPlanMode,
        },
        model: agent
          ? {
              provider: chatModel.view?.primary.provider ?? agent.provider,
              current: chatModel.view?.primary.model ?? agent.model,
              busy: activeSession
                ? chatModel.busy || !chatModel.view
                : savingSettings,
              error: activeSession ? chatModel.error : settingsSaveError,
              choices:
                activeSession && chatModel.view
                  ? chatModel.view.availableModels
                      .filter(
                        (choice) =>
                          choice.provider === chatModel.view?.primary.provider,
                      )
                      .map((choice) => choice.model)
                  : undefined,
              onPick: (model: string) => {
                if (activeSession && chatModel.view)
                  void chatModel.update({
                    mode: 'override',
                    provider: chatModel.view.primary.provider,
                    model,
                  });
                else chooseChatModel();
              },
              ...(activeSession && chatModel.view
                ? (() => {
                    const view = chatModel.view;
                    const primary = view.primary;
                    const efforts =
                      view.availableModels.find(
                        (choice) =>
                          choice.provider === primary.provider &&
                          choice.model === primary.model,
                      )?.reasoningEfforts ?? [];
                    return {
                      efforts,
                      effort: primary.reasoningEffort ?? null,
                      onEffort: (effort: string | null) =>
                        void chatModel.update({
                          mode: 'override',
                          provider: primary.provider,
                          model: primary.model,
                          ...(effort
                            ? { reasoningEffort: effort as ChatReasoningEffort }
                            : {}),
                        }),
                    };
                  })()
                : {}),
              onMore: () => navigate({ kind: 'page', page: 'model' }),
              moreLabel: 'All models in Settings'
            }
          : undefined,
        setDraft,
        sending,
        // Usable while the companion works: messages queue (spec §15.3).
        disabled: resetting || sessionLoading || waitingForMain || daemonTooOld,
        offline: connection === 'offline',
        onSend: commands.send,
        documentEditContext: chat.resend?.documentEditContext,
        outcome: chat.resend?.outcome,
        onClearDocumentEditContext: clearDocumentEditContext,
        error: workspaceError,
        onDismissError: () => setWorkspaceError(null),
        commands: chat.literalDraft ? undefined : slashCommands,
        runActive: activeRun !== null,
        onStop:
          activeRun && activeSession?.capabilities.stop
            ? () => void stopRun(activeRun)
            : undefined,
        onSteer:
          activeRun && activeSession?.capabilities.steer
            ? commands.steer
            : undefined,
        recovery:
          failedDrafts.length > 0 && !sending
            ? {
                count: failedDrafts.length,
                text: failedDraft ?? '',
                // While the chat is still loading, a restore would be
                // dropped; Restore waits for it instead.
                ready:
                  agent !== null &&
                  !resetting &&
                  !sessionLoading &&
                  !waitingForMain,
                restore: () => {
                  if (!activeChatKey) return;
                  const recovered = failedDrafts[0];
                  if (recovered?.documentEditContext && (!activeSession || sessionLoading || waitingForMain)) return;
                  if (recovered && ((recovered.documentEditContext !== undefined && (!validPreviewEditContext(recovered.documentEditContext) || activeSession?.kind !== 'chat' || recovered.agentId !== activeSession.agentId || recovered.sessionId !== activeSession.id)) || (!recovered.documentEditContext && recovered.text.includes('BEGIN_PREVIEW_EDIT_CONTEXT')))) {
                    updateChat(activeChatKey, (current) => ({ failedDrafts: current.failedDrafts.slice(1), error: 'This recovered edit needs a current selection. Open the original file and select its content again.' })); return;
                  }
                  if (
                    !attachments.restore(
                      failedDrafts[0]?.attachments ?? [],
                      failedDrafts[0]?.attachmentIds,
                    )
                  )
                    return;
                  updateChat(activeChatKey, (current) => {
                    const [first, ...rest] = current.failedDrafts;
                    if (!first) return {};
                    return {
                      draft: current.draft.trim()
                        ? `${current.draft}\n\n${first.text}`
                        : first.text,
                      failedDrafts: rest,
                      resend:
                        first.idempotencyKey ||
                        first.attachmentIds?.length ||
                        first.skill ||
                        first.documentEditContext || first.outcome
                          ? {
                              text: first.text,
                              idempotencyKey:
                                first.idempotencyKey ?? crypto.randomUUID(),
                              ...(first.skill ? { skill: first.skill } : {}),
                              ...(first.documentEditContext ? { documentEditContext: first.documentEditContext } : {}),
                              ...(first.outcome ? { outcome: first.outcome } : {}),
                              ...(first.attachmentIds
                                ? { attachmentIds: first.attachmentIds }
                                : {}),
                            }
                          : current.resend,
                    };
                  });
                },
                dismiss: () => setFailedDrafts((current) => current.slice(1)),
              }
            : undefined,
      }}
      onNewChat={newChat}
      onOpenWork={() => navigate({ kind: 'page', page: 'work' })}
      onRename={(title) =>
        activeSession
          ? renameSession(activeSession, title)
          : Promise.resolve(false)
      }
      onToggleArchived={() => {
        if (activeSession)
          void archiveSession(activeSession, !activeSession.archived);
      }}
      onExport={() => {
        if (activeSession) void exportSession(activeSession);
      }}
      controls={{
        revising: commands.revising,
        onRevise: commands.reviseSession,
        onQueuePatch: commands.updateQueuedRun,
        onCancel: (run) => void commands.stopRun(run),
        onOpenSource: (sessionId) =>
          commands.openTarget({
            agentId: activeSession?.agentId ?? agent.id,
            sessionId,
          }),
      }}
      notice={
        <>
          {chat.notice && (
            <p role="status" className="px-4 pt-3 text-xs text-ink-3">
              {chat.notice}
            </p>
          )}
          {waitingForMain && mainChat.error ? (
            <div role="alert" className="px-4 pt-3 text-xs text-danger">
              <p>Main chat could not be loaded: {mainChat.error}</p>
              <button
                type="button"
                className="mt-2 underline"
                onClick={mainChat.retry}
              >
                Retry main chat
              </button>
            </div>
          ) : null}
          {legacyMigrationError ? (
            <p role="status" className="px-4 pt-3 text-xs text-ink-3">
              {legacyMigrationError}
            </p>
          ) : null}
          {daemonTooOld ? (
            <div
              role="alert"
              className="mx-4 mt-3 rounded-xl border border-danger/25 bg-danger/[0.08] px-3.5 py-2.5 text-xs leading-relaxed"
            >
              <p className="font-semibold text-danger">Update the daemon</p>
              <p className="text-ink-2">
                {sessions.daemonTooOld
                  ? 'This console keeps chats as sessions'
                  : 'This console sends messages as live runs'}
                , which this anima-daemon does not support yet. Update and
                restart the daemon, then reload this page.
              </p>
            </div>
          ) : null}
          {/* A dropped stream says so until its next snapshot (spec §16);
           *  the region stays, so a retry that fails again reads nothing. */}
          <div role="status">
            {live.status === 'reconnecting' ? (
              <p className="px-4 pt-3 text-center font-mono text-[10px] text-ink-3">
                Reconnecting…
              </p>
            ) : null}
          </div>
          {chat.delivery?.queued ? (
            <p
              role="status"
              className="px-4 pt-3 text-center font-mono text-[10px] text-mint"
            >
              Queued for Telegram delivery
            </p>
          ) : null}
          {sessionActionError ? (
            <p role="alert" className="px-4 pt-3 text-xs text-danger">
              {sessionActionError}
            </p>
          ) : null}
          {sessionLoading && sessionReadError ? (
            <p role="alert" className="px-4 pt-3 text-xs text-danger">
              Session details could not be loaded: {sessionReadError}. Retrying…
            </p>
          ) : null}
          {routeSessionId && !routeMessageId && history.error ? (
            <p role="alert" className="px-4 pt-3 text-xs text-danger">
              Messages could not be loaded: {history.error}
            </p>
          ) : null}
        </>
      }
    />
  );

  return (
    <>
      <div data-testid="workspace-background" className="contents">
        <WorkspaceShell
          projectPage={route.kind === 'project'
            ? <ProjectHome agentId={agent.id} agents={agents} projectId={route.projectId} onSelectAgent={selectAgent} navigate={navigate} />
            : <span hidden />}
          mainAgent={mainAgent ?? agent}
          settingsAgent={agent}
          agents={agents}
          connection={connection}
          route={route}
          navigate={navigate}
          onNewChat={newChat}
          searchMessages={
            agentId && connection === 'online' ? searchMessages : undefined
          }
          searchMemories={
            agentId && connection === 'online' ? searchMemories : undefined
          }
          onOpenMessage={openMessage}
          onSelectAgent={selectAgent}
          newAgent={
            <CompanionSetup
              providers={providers}
              providersError={providersError}
              retryProviders={retryProviders}
              onCreated={(snapshot) => {
                acceptAgentSnapshot(snapshot);
                selectAgent(snapshot.state.id);
                navigate({ kind: 'home' });
                void refreshWorkspace();
              }}
            />
          }
          onOpenReport={agent ? () => setReportOpen(true) : undefined}
          onExportConversation={
            activeSession?.capabilities.export
              ? () => void exportSession(activeSession)
              : undefined
          }
          onShareConversation={
            reviewsChat(activeSession)
              ? () => setSharing(activeSession)
              : undefined
          }
          inbox={
            agent
              ? {
                  count: needsYou.items.length,
                  render: (close) => (
                    <NeedsYouPanel
                      items={needsYou.items}
                      agentName={agent.name}
                      decideApproval={decideApproval}
                      approvePairing={integrations.approvePairing}
                      openSession={(targetAgentId, sessionId) =>
                        commands.openTarget({
                          agentId: targetAgentId,
                          sessionId,
                        })
                      }
                      openPage={(page) => navigate({ kind: 'page', page })}
                      done={needsYou.done}
                      onShowDone={() => void needsYou.loadDone()}
                      onClose={close}
                      onOpenReport={() => setReportOpen(true)}
                    />
                  ),
                }
              : null
          }
          locked={settingsBusy}
          profile={settingsPage(['profile'])}
          model={settingsPage(['model'])}
          builtinTools={settingsPage(['builtins'])}
          runtime={
            <RuntimePage
              agent={agent}
              workspace={workspace}
              online={connection === 'online'}
            />
          }
          onChangeWorkspaceAvatar={changeWorkspaceAvatar}
          onPickPrompt={(prompt) =>
            setDraft((current) =>
              current.trim() ? `${current}\n\n${prompt}` : prompt,
            )
          }
          connectors={(mode) => (
            <ConnectorsView
              mode={mode}
              agentId={agent.id}
              agentName={agent.name}
              online={connection === 'online'}
              epoch={live.state.epoch}
              disabled={
                savingSettings ||
                resetting ||
                mcpBusy ||
                agent.workspaceRole === 'helper'
              }
              mcpPermissionsUncertain={mcpPermissionsUnknownRef.current}
              mcpPermissionsBusy={mcpBusy}
              onMcpBusyChanged={(busy) => {
                if (
                  !connectorLifetime.isCurrent(connectorLifetime.token) ||
                  currentAgentIdRef.current !== agent.id ||
                  agentLifecycleGenerationRef.current !==
                    onboardingLifecycleGeneration
                )
                  return;
                mcpBusyRef.current = busy;
                setMcpBusy(busy);
              }}
              onMcpGrantsChanged={async () => {
                if (
                  !connectorLifetime.isCurrent(connectorLifetime.token) ||
                  currentAgentIdRef.current !== agent.id ||
                  agentLifecycleGenerationRef.current !==
                    onboardingLifecycleGeneration
                )
                  return;
                const operation = beginAgentOperation(agent.id);
                let canonical: DaemonSnapshot;
                try {
                  canonical = (await daemon.getAgent(agent.id)).agent;
                } catch (caught) {
                  if (
                    connectorLifetime.isCurrent(connectorLifetime.token) &&
                    isCurrentAgentOperation(operation)
                  ) {
                    mcpPermissionsUnknownRef.current = true;
                    setSettingsSaveError(
                      'External tool permissions need a fresh read. Refresh status in Connections before saving changes.',
                    );
                  }
                  throw caught;
                }
                if (
                  !connectorLifetime.isCurrent(connectorLifetime.token) ||
                  !isCurrentAgentOperation(operation)
                )
                  return;
                if (canonical.state.id !== agent.id) {
                  mcpPermissionsUnknownRef.current = true;
                  setSettingsSaveError(
                    'External tool permissions need a fresh read. Refresh status in Connections before saving changes.',
                  );
                  throw new Error('Permission read did not match this agent.');
                }
                // Only permissions are reconciled. A simultaneous poll may have
                // newer name/model/system fields than the owner read above.
                acceptAgentTools(agent.id, canonical.state.config.tools);
                mcpPermissionsUnknownRef.current = false;
                setSettingsSaveError(null);
              }}
              onMcpPermissionsUncertain={() => {
                if (
                  !connectorLifetime.isCurrent(connectorLifetime.token) ||
                  currentAgentIdRef.current !== agent.id ||
                  agentLifecycleGenerationRef.current !==
                    onboardingLifecycleGeneration
                )
                  return;
                mcpPermissionsUnknownRef.current = true;
                setSettingsSaveError(
                  'External tool permissions need a fresh read. Refresh status in Connections before saving changes.',
                );
              }}
              telegramStatus={
                telegramConnector
                  ? connectorStatusLabel(telegramConnector.status)
                  : null
              }
              discordStatus={
                discordIntegrations.connectors.length === 0
                  ? null
                  : discordIntegrations.connectors.some(
                        (item) => item.status === 'ready',
                      )
                    ? 'Connected'
                    : discordStatusLabel(discordIntegrations.connectors[0].status)
              }
              telegram={
                <TelegramSettings
                  online={connection === 'online'}
                  epoch={live.state.epoch}
                  disabled={
                    savingSettings ||
                    resetting ||
                    agent.workspaceRole === 'helper'
                  }
                  connector={telegramConnector}
                  busy={integrations.connectorBusy}
                  error={integrations.connectorError}
                  connect={integrations.connectTelegram}
                  replace={integrations.replaceTelegram}
                  approve={integrations.approvePairing}
                  restart={integrations.restartTelegram}
                  disconnect={integrations.disconnectTelegram}
                  refresh={integrations.refresh}
                />
              }
              discord={
                <DiscordSettings
                  agentId={agent.id}
                  online={connection === 'online'}
                  epoch={live.state.epoch}
                  disabled={
                    savingSettings ||
                    resetting ||
                    agent.workspaceRole === 'helper'
                  }
                  {...discordIntegrations}
                  openConversation={(connector, roomId) =>
                    commands.openTarget({
                      agentId: connector.agentId,
                      sessionId: roomId ?? connector.roomId,
                    })
                  }
                />
              }
            />
          )}
          approvals={
            <ApprovalsPage
              agentId={agent.id}
              agentName={agent.name}
              live={live.state}
              streamOpen={live.status === 'open'}
              sessions={sessions.sessions}
              onOpenSession={(approval) =>
                commands.openTarget({
                  agentId: approval.agentId,
                  sessionId: approval.sessionId,
                })
              }
              access={settingsPage(['access'])}
            />
          }
          automations={
            <AutomationsPage
              view={automations}
              agentId={agent.id}
              version={live.state.automationsVersion}
              online={connection === 'online'}
              telegramConnectorId={
                telegramConnector?.approvedChat ? telegramConnector.id : null
              }
              discordDestinations={
                discordIntegrations.available
                  ? discordIntegrations.connectors
                      .filter(
                        (connector) =>
                          discordCanReply(connector) &&
                          connector.settings?.homeChannelId,
                      )
                      .map((connector) => ({
                        connectorId: connector.id,
                        label: `${connector.bot.displayName ?? connector.bot.username ?? 'Discord bot'} · #${connector.conversations?.find((conversation) => conversation.channelId === connector.settings?.homeChannelId)?.name ?? connector.settings!.homeChannelId}`,
                      }))
                  : []
              }
              focusId={automationFocus}
              onFocusHandled={clearAutomationFocus}
              onShowInChat={showAutomationInChat}
            />
          }
          memory={
            <MemoryPage
              agentId={agent.id}
              online={connection === 'online'}
              epoch={live.state.epoch}
            />
          }
          usage={
            <UsagePage
              agentId={null}
              online={connection === 'online'}
              epoch={live.state.epoch}
              sessionId={null}
              allAgents
              agents={agents}
            />
          }
          logs={<LogsPage online={connection === 'online'} />}
          health={
            <HealthPage
              online={connection === 'online'}
              epoch={live.state.epoch}
            />
          }
          skills={
            <SkillsPage
              version={live.state.skillsVersion}
              epoch={live.state.epoch}
              online={connection === 'online'}
              onPendingCountChange={reportPendingSkillCount}
              onOpenSession={(proposer) =>
                commands.openTarget({
                  agentId: proposer.agentId,
                  sessionId: proposer.sessionId,
                })
              }
              learning={
                <LearningReviewPanel
                  agentId={agent.id}
                  api={learningApi}
                  disabled={settingsBusy || mcpBusy}
                />
              }
            />
          }
          pendingApprovals={pendingApprovalCount(
            live.state.approvals,
            live.status === 'open',
            sessions.sessions,
          )}
          pendingSkills={pendingSkillCounts[agent.id] ?? 0}
          sessions={sessions.sessions}
          onOpenSession={openSession}
          workspaceState={workspace}
          sidebar={sidebar}
          conversation={sessionView}
          conversationRoute={conversationRoute}
        />
      </div>
      <AutomationToasts
        toasts={automationToasts.toasts}
        automations={automations.automations}
        loaded={automations.loaded}
        online={connection === 'online'}
        onDismiss={automationToasts.dismiss}
        onOpen={openToast}
        onDone={automations.complete}
        onSnooze={automations.snooze}
      />
      {movingProjectChat && <MoveChatDialog key={`${movingProjectChat.agentId}:${movingProjectChat.id}`}
        session={movingProjectChat} onClose={closeProjectMove} onMoved={() => {
          sessions.refresh(); setSessionRereads((value) => value + 1);
        }} />}
      {sharing && (
        <ShareConversationDialog
          agentId={sharing.agentId}
          sessionId={sharing.id}
          title={sharing.title}
          clearGeneration={sharing.clearGeneration ?? 0}
          onClose={() => setSharing(null)}
        />
      )}
      {reportOpen && agent && (
        <WeeklyReport
          agentId={agent.id}
          agentName={agent.name}
          sessions={sessions.sessions}
          openSession={(sessionId) =>
            commands.openTarget({ agentId: agent.id, sessionId })
          }
          onClose={() => setReportOpen(false)}
        />
      )}
    </>
  );
}

import {
  memo,
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { ConversationTools } from './ConversationTools';
import { HaloOrb } from './halo/HaloOrb';
import { usePresenceSignals } from '../lib/presence';
import { CopyMessage } from './CopyMessage';
import { SaveToMemory } from './memory/SaveToMemory';
import type { SaveOutcome, SavedState } from '../hooks/useSaveToMemory';
import type { AgentDetail, ChatMessage } from '../lib/types';
import {
  AlertIcon,
  BoltIcon,
  PlusIcon,
  PulseIcon,
  SendIcon,
  StopIcon,
} from './icons';
import { MarkdownMessage } from './MarkdownMessage';
import { ErrorBanner, formatTime } from './ui-bits';
import { isMacPlatform } from '../lib/platform';
import {
  buildTranscript,
  isAcceptedBudgetWrapupMessage,
  type TranscriptActions,
  type TranscriptItem,
} from '../lib/transcript';
import { PendingMessage, RunActivity, ToolBlock } from './sessions/RunActivity';
import { RunOutcomeCard } from './sessions/RunOutcomeCard';
import { TaskReviewAction } from './learning/TaskReviewAction';
import { TaskReviewProvider } from './learning/TaskReviewContext';
import { DelegatedTurn, TrimmedDivider } from './sessions/TranscriptNotes';
import { slashSuggestions, type SlashCommand } from '../lib/slash-commands';
import { SlashCommandMenu } from './sessions/SlashCommandMenu';
import {
  AttachmentControls,
  AttachmentDropZone,
} from './attachments/AttachmentControls';
import { AttachmentChips } from './attachments/AttachmentChips';
import { MessageAttachments } from './attachments/MessageAttachments';
import type { AttachmentDraft } from '../lib/attachments';
import { DictationControl } from './voice/DictationControl';
import { MicrophoneCapture } from './voice/MicrophoneCapture';
import {
  NativeTranscription,
  type NativeVoiceScope,
} from './voice/NativeTranscription';
import { ReadAloud } from './voice/ReadAloud';
import type { ReadAloudController } from '../hooks/useReadAloud';
import { revealInvisible } from '../lib/skills';
import {
  EMPTY_RUN_ATTACHMENTS,
  type KnownRunAttachments,
} from '../lib/run-attachments';

/* ── Messages ── */
function EventPill({ message }: { message: ChatMessage }) {
  const text = message.content.text;
  return (
    <div className="animate-fade-in flex justify-center">
      <span
        className="max-w-md truncate rounded-full border border-line bg-white/[0.02] px-3 py-1 font-mono text-[10px] text-ink-3"
        title={text}
      >
        {message.role.toLowerCase()} · {text}
      </span>
    </div>
  );
}

/** Why a reply is partial (spec §4.5, §4.6). */
function messageFlag(message: ChatMessage): string | null {
  const metadata = message.content.metadata;
  if (metadata?.stopped === true) return 'Stopped';
  if (metadata?.incomplete === true) return 'Incomplete';
  return null;
}

/** Where an owner message came from, when not this console: the owner's
 *  private Telegram chat shares the main chat. */
const SOURCE_LABELS: Record<string, string> = {
  telegram: 'Telegram',
  discord: 'Discord',
};
function messageSource(message: ChatMessage): string | null {
  if (message.role !== 'User') return null;
  const source = message.content.metadata?.source;
  return typeof source === 'string' ? (SOURCE_LABELS[source] ?? null) : null;
}

/** Memoized on its message: a committed message renders once, however
 *  often the transcript around it changes (a streamed reply's deltas). */
const Bubble = memo(function Bubble({
  message,
  onSave,
  savedLookup,
  agentId,
  agentName = '',
  readAloud,
}: {
  message: ChatMessage;
  /** Shown beside assistant replies so every reply is attributed. */
  agentName?: string;
  /** Offers Save to memory on the owner's own messages: the agent decides
   *  what of its replies is worth keeping, and the owner can say so. */
  onSave?: (message: ChatMessage) => Promise<SaveOutcome>;
  savedLookup?: (messageId: string) => SavedState | null;
  agentId: string;
  readAloud?: ReadAloudController;
}) {
  if (message.role !== 'User' && message.role !== 'Assistant') {
    return <EventPill message={message} />;
  }
  const isUser = message.role === 'User';
  const flag = messageFlag(message);
  const time = formatTime(message.created_at_ms);
  // A reply is a turn under the companion's name; the owner's words sit in
  // a slim bubble on the right. Actions stay out of the way until wanted.
  return (
    <div
      className={`chat-turn min-w-0 max-w-full animate-msg-in ${isUser ? 'is-user' : 'is-agent'}`}
    >
      {!isUser && (
        <div className="chat-agent-head">
          {agentName && <span className="chat-agent-name">{agentName}</span>}
          <span className="chat-time">{time}</span>
          {flag && <span className="message-flag">{flag}</span>}
        </div>
      )}
      <div
        className={`chat-body min-w-0 max-w-full break-words ${
          isUser
            ? 'scenic-user-bubble border border-line-strong bg-panel-2/90 text-ink'
            : 'scenic-agent-text text-ink'
        }`}
      >
        <MarkdownMessage>
          {revealInvisible(message.content.text).text}
        </MarkdownMessage>
        {message.content.attachments && (
          <MessageAttachments
            agentId={agentId}
            attachments={message.content.attachments}
          />
        )}
      </div>
      {messageSource(message) && (
        <span className="chat-source">via {messageSource(message)}</span>
      )}
      <div className="chat-actions studio-message-meta">
        {isUser && <span className="chat-time">{time}</span>}
        {isUser && flag && <span className="message-flag">{flag}</span>}
        <CopyMessage text={message.content.text} />
        {!isUser && readAloud && (
          <ReadAloud
            messageId={message.id}
            text={message.content.text}
            controller={readAloud}
          />
        )}
        {onSave &&
          message.role === 'User' &&
          message.content.text.trim() !== '' && (
            <SaveToMemory
              message={message}
              save={onSave}
              saved={savedLookup?.(message.id) ?? null}
            />
          )}
      </div>
    </div>
  );
});

const SUGGESTIONS = [
  {
    icon: <Sparkle />,
    text: 'What can you do for me?',
    title: 'Explore what I can do',
    label: 'DISCOVER',
    detail: 'Understand the agent’s tools and access.',
  },
  {
    icon: <BoltIcon size={13} />,
    text: 'Help me plan my day',
    title: 'Make space for your day',
    label: 'MAKE A PLAN',
    detail: 'Define the objective and next steps.',
  },
  {
    icon: <PulseIcon size={13} />,
    text: 'Check in on me every hour',
    title: 'Set a check-in',
    label: 'STAY IN SYNC',
    detail: 'Agree on a schedule for progress updates.',
  },
];

function Sparkle() {
  return <BoltIcon size={13} />;
}

function greetingForNow(): string {
  const hour = new Date().getHours();
  if (hour < 12) return 'Good morning.';
  if (hour < 18) return 'Good afternoon.';
  return 'Good evening.';
}

function EmptyState({
  agentName,
  onPick,
}: {
  agentName: string;
  onPick: (text: string) => void;
}) {
  return (
    <div className="studio-welcome scenic-welcome animate-rise-in">
      <h2
        aria-label={`Say something to ${agentName}`}
        className="studio-hero-title"
      >
        {greetingForNow()}
      </h2>
      <p className="studio-hero-description">
        Talk or type. {agentName} is listening.
      </p>
      <div className="studio-suggestions">
        {SUGGESTIONS.map((s) => (
          <button
            key={s.text}
            aria-label={`${s.title}. ${s.text}`}
            onClick={() => onPick(s.text)}
            className="studio-suggestion group text-left"
          >
            <span className="studio-suggestion-icon" aria-hidden>
              {s.icon}
            </span>
            <span>{s.text}</span>
          </button>
        ))}
      </div>
    </div>
  );
}

function ThinkingIndicator({ name }: { name: string }) {
  return (
    <div className="chat-thinking animate-fade-in">
      <span className="chat-thinking-orb agent-orb" aria-hidden />
      <div className="flex items-center gap-1.5" aria-hidden>
        {[0, 1, 2].map((i) => (
          <span
            key={i}
            className="typing-dot h-1.5 w-1.5 rounded-full bg-accent"
            style={{ animationDelay: `${i * 150}ms` }}
          />
        ))}
      </div>
      <span className="chat-thinking-label text-shimmer">
        {name} is thinking
      </span>
    </div>
  );
}

function candidateAnchorIds(item: TranscriptItem): string[] {
  switch (item.kind) {
    case 'message':
    case 'delegated':
    case 'revised':
      return [item.message.id];
    case 'tools':
      return item.messageIds;
    default:
      return [];
  }
}

/**
 * The ids each transcript item owns for scroll-jump and search-highlight
 * (spec §15.3), one array per item in `transcript` order. A message with
 * both text and tool calls produces two items that both cite its id (its
 * bubble and its tools block); exactly one may claim it, or the jump
 * target and the highlighted item become ambiguous and unmounting either
 * item deletes the id mapping the other still needs. The text bubble
 * (pushed first, see `buildTranscript`) wins.
 */
function anchorsFor(transcript: readonly TranscriptItem[]): string[][] {
  const claimed = new Set<string>();
  return transcript.map((item) => {
    const owned = candidateAnchorIds(item).filter((id) => !claimed.has(id));
    for (const id of owned) claimed.add(id);
    return owned;
  });
}

/** When a transcript item happened, for day dividers; live items have none. */
function itemTime(item: TranscriptItem): number | null {
  return item.kind === 'message' ||
    item.kind === 'delegated' ||
    item.kind === 'revised'
    ? item.message.created_at_ms
    : null;
}

function dayLabel(ms: number, now = new Date()): string {
  const day = new Date(ms);
  const startOf = (d: Date) =>
    new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const diff = Math.round((startOf(now) - startOf(day)) / 86_400_000);
  if (diff === 0) return 'Today';
  if (diff === 1) return 'Yesterday';
  return day.toLocaleDateString([], {
    weekday: 'short',
    day: 'numeric',
    month: 'short',
    ...(day.getFullYear() === now.getFullYear() ? {} : { year: 'numeric' }),
  });
}

/** The main chat runs for days: a divider marks where each new day starts. */
function dayDividers(transcript: readonly TranscriptItem[]): (string | null)[] {
  let previous: string | null = null;
  return transcript.map((item) => {
    const ms = itemTime(item);
    if (ms === null) return null;
    const day = new Date(ms).toDateString();
    const changed = previous !== null && day !== previous;
    previous = day;
    return changed ? dayLabel(ms) : null;
  });
}

/** Memoized on its item: history items keep their identity while a run
 *  streams, so only the items that changed render again. */
const TranscriptEntry = memo(function TranscriptEntry({
  item,
  agentName,
  actions,
  agentId,
  readAloud,
  knownAttachments,
}: {
  item: TranscriptItem;
  agentName: string;
  actions?: TranscriptActions;
  agentId: string;
  readAloud?: ReadAloudController;
  knownAttachments: KnownRunAttachments;
}) {
  const renderBubble = (message: ChatMessage) => (
    <Bubble
      message={message}
      agentId={agentId}
      agentName={agentName}
      readAloud={readAloud}
    />
  );
  switch (item.kind) {
    case 'message':
      return (
        <>
          <Bubble
            message={item.message}
            onSave={actions?.onSaveToMemory}
            savedLookup={actions?.savedToMemory}
            agentId={agentId}
            agentName={agentName}
            readAloud={readAloud}
          />
          {isAcceptedBudgetWrapupMessage(item.message) && (
            <div className="mt-2 max-w-[85%] rounded-xl border border-line bg-panel-2/90 px-4 py-3 text-sm text-ink-2">
              <h3 className="font-medium text-ink">Tool limit reached</h3>
              <p className="mt-1">
                This saved reply could not run further tools. Review the tool
                results and any remaining work.
              </p>
            </div>
          )}
        </>
      );
    case 'revised':
      return (
        <details className="revised-draft">
          <summary>Earlier draft (revised)</summary>
          <Bubble
            message={item.message}
            agentId={agentId}
            agentName={agentName}
          />
        </details>
      );
    case 'delegated':
      return (
        <DelegatedTurn from={item.from} text={item.message.content.text} />
      );
    case 'tools':
      return <ToolBlock steps={item.steps} active={false} actions={actions} />;
    case 'run':
      return (
        <RunActivity
          live={item.live}
          agentName={agentName}
          actions={actions}
          renderMessage={renderBubble}
          knownAttachments={knownAttachments}
        />
      );
    case 'outcome':
      return (
        <RunOutcomeCard
          run={item.run}
          onSendAgain={actions?.onSendAgain}
          resent={actions?.resentRunIds?.has(item.run.id) ?? false}
        />
      );
    case 'learning-review':
      return <TaskReviewAction run={item.run} />;
    case 'pending':
      return (
        <PendingMessage
          pending={item.pending}
          onCancel={actions?.onCancelPending}
          renderMessage={renderBubble}
        />
      );
    case 'trimmed':
      return (
        <TrimmedDivider
          onCompact={actions?.onCompact}
          compacting={actions?.compacting}
        />
      );
  }
});

export const MessageList = memo(function MessageList({
  agent,
  sending,
  scrollerRef,
  onSuggestion,
  hasOlder = false,
  loadingOlder = false,
  onLoadOlder,
  emptyState,
  items,
  actions,
  readAloud,
  knownAttachments = EMPTY_RUN_ATTACHMENTS,
  highlightMessageId = null,
  jumpRequestKey = 0,
  jumpReady = true,
  onJumpToLatest,
}: {
  agent: AgentDetail;
  sending: boolean;
  scrollerRef: React.RefObject<HTMLDivElement | null>;
  onSuggestion: (text: string) => void;
  /** Older history exists: show a control and load it on scroll to the top. */
  hasOlder?: boolean;
  loadingOlder?: boolean;
  onLoadOlder?: () => void;
  /** Replaces the welcome screen for sessions that are not new chats. */
  emptyState?: ReactNode;
  /** The session's transcript with its live runs and sends (spec §15.2);
   *  built from `agent.messages` when absent. */
  items?: readonly TranscriptItem[];
  actions?: TranscriptActions;
  readAloud?: ReadAloudController;
  knownAttachments?: KnownRunAttachments;
  /** A bounded history result; waits for its transcript anchor to mount. */
  highlightMessageId?: string | null;
  jumpRequestKey?: number;
  /** The requested historical window has finished loading successfully. */
  jumpReady?: boolean;
  onJumpToLatest?: () => void;
}) {
  const [awayFromBottom, setAwayFromBottom] = useState(false);
  const [highlight, setHighlight] = useState<string | null>(null);
  const highlightedMessageId =
    highlight ?? (jumpReady ? highlightMessageId : null);
  const atBottom = useRef(true);
  const messageElements = useRef(new Map<string, HTMLDivElement>());
  const firstMessageId = agent.messages[0]?.id;
  const anchor = useRef<{ firstId: string | undefined; height: number }>({
    firstId: firstMessageId,
    height: 0,
  });
  const externalJump = useRef<{
    messageId: string | null;
    requestKey: number;
    agentId: string;
    done: boolean;
  }>({
    messageId: null,
    requestKey: 0,
    agentId: agent.id,
    done: false,
  });
  const transcript = useMemo(
    () => items ?? buildTranscript({ messages: agent.messages }),
    [items, agent.messages],
  );
  const anchorsByItem = useMemo(() => anchorsFor(transcript), [transcript]);
  const showsReviews = useMemo(
    () => transcript.some((item) => item.kind === 'learning-review'),
    [transcript],
  );
  const days = useMemo(() => dayDividers(transcript), [transcript]);
  const jumpToMessage = useCallback((id: string) => {
    atBottom.current = false;
    setAwayFromBottom(true);
    messageElements.current.get(id)?.scrollIntoView?.({ block: 'center' });
  }, []);
  const jumpToLatest = () => {
    if (highlightMessageId && onJumpToLatest) {
      onJumpToLatest();
      return;
    }
    const element = scrollerRef.current;
    if (element) element.scrollTop = element.scrollHeight;
    atBottom.current = true;
    setAwayFromBottom(false);
  };
  useLayoutEffect(() => {
    const previous = externalJump.current;
    const changed =
      previous.messageId !== highlightMessageId ||
      previous.requestKey !== jumpRequestKey ||
      previous.agentId !== agent.id;
    if (changed) {
      externalJump.current = {
        messageId: highlightMessageId,
        requestKey: jumpRequestKey,
        agentId: agent.id,
        done: false,
      };
      setHighlight(null);
      if (!highlightMessageId && previous.messageId) {
        atBottom.current = true;
        setAwayFromBottom(false);
        const scroller = scrollerRef.current;
        if (scroller) scroller.scrollTop = scroller.scrollHeight;
      }
    }
    if (!highlightMessageId) return;
    // Disable bottom-follow even while the requested window is loading.
    atBottom.current = false;
    setAwayFromBottom(true);
    if (!jumpReady || externalJump.current.done) return;
    const element = messageElements.current.get(highlightMessageId);
    if (!element) return;
    externalJump.current.done = true;
    element.scrollIntoView?.({ block: 'center' });
    // The ordinary prepend-preservation effect below must not offset the
    // position we just chose when the anchor window replaced the page.
    anchor.current = {
      firstId: firstMessageId,
      height: scrollerRef.current?.scrollHeight ?? 0,
    };
  }, [
    highlightMessageId,
    jumpRequestKey,
    jumpReady,
    agent.id,
    transcript,
    firstMessageId,
    scrollerRef,
  ]);
  useLayoutEffect(() => {
    if (atBottom.current) {
      const element = scrollerRef.current;
      if (element) element.scrollTop = element.scrollHeight;
    }
  }, [transcript, sending, scrollerRef]);
  // Keep the reading position when older messages are prepended.
  useLayoutEffect(() => {
    const element = scrollerRef.current;
    if (!element) return;
    const previous = anchor.current;
    if (
      previous.firstId !== undefined &&
      previous.firstId !== firstMessageId &&
      !atBottom.current
    ) {
      element.scrollTop += element.scrollHeight - previous.height;
    }
    anchor.current = { firstId: firstMessageId, height: element.scrollHeight };
  }, [firstMessageId, agent.messages, scrollerRef]);

  return (
    <>
      {agent.messages.length > 0 && (
        <ConversationTools
          agent={agent}
          onJump={jumpToMessage}
          onHighlight={setHighlight}
        />
      )}
      <div className="studio-conversation-body">
        <div
          ref={scrollerRef}
          className="studio-message-scroller relative z-[1] min-h-0 flex-1 overflow-y-auto"
          aria-label={`Conversation with ${agent.name}`}
          onScroll={(event) => {
            const element = event.currentTarget;
            atBottom.current =
              element.scrollHeight - element.scrollTop - element.clientHeight <
              80;
            setAwayFromBottom(!atBottom.current);
            if (
              element.scrollTop < 40 &&
              hasOlder &&
              !loadingOlder &&
              !highlightMessageId
            ) {
              onLoadOlder?.();
            }
          }}
        >
          {transcript.length === 0 && !sending ? (
            (emptyState ?? (
              <EmptyState agentName={agent.name} onPick={onSuggestion} />
            ))
          ) : (
            <TaskReviewProvider
              agentId={agent.id}
              active={showsReviews}
              api={actions?.taskReviews}
            >
              <div className="studio-messages mx-auto flex w-full max-w-3xl flex-col gap-4 px-4 py-6 sm:px-6">
                {hasOlder && onLoadOlder && (
                  <button
                    type="button"
                    className="studio-tool-button session-load-older"
                    onClick={onLoadOlder}
                    disabled={loadingOlder}
                  >
                    {loadingOlder
                      ? 'Loading older messages…'
                      : 'Load older messages'}
                  </button>
                )}
                {transcript.map((item, index) => {
                  const ids = anchorsByItem[index];
                  // This item's own last-registered element, so cleanup
                  // only ever removes what THIS ref put there — never a
                  // different item's registration for the same id.
                  let ownElement: HTMLDivElement | null = null;
                  return (
                    <div
                      key={item.key}
                      ref={(element) => {
                        if (element) {
                          ownElement = element;
                          for (const id of ids)
                            messageElements.current.set(id, element);
                        } else {
                          for (const id of ids)
                            if (messageElements.current.get(id) === ownElement)
                              messageElements.current.delete(id);
                          ownElement = null;
                        }
                      }}
                      data-search-match={
                        (highlightedMessageId !== null &&
                          ids.includes(highlightedMessageId)) ||
                        undefined
                      }
                      className="studio-message-anchor"
                    >
                      {days[index] && (
                        <div className="chat-day" role="separator">
                          <span>{days[index]}</span>
                        </div>
                      )}
                      <TranscriptEntry
                        item={item}
                        agentName={agent.name}
                        actions={actions}
                        agentId={agent.id}
                        readAloud={readAloud}
                        knownAttachments={knownAttachments}
                      />
                    </div>
                  );
                })}
                {sending && <ThinkingIndicator name={agent.name} />}
              </div>
            </TaskReviewProvider>
          )}
        </div>
        {awayFromBottom && (
          <button
            type="button"
            className="studio-jump-latest"
            onClick={jumpToLatest}
          >
            ↓ Jump to latest
          </button>
        )}
      </div>
    </>
  );
});

/* ── Composer ── */
export function Composer({
  agentName,
  label,
  draft,
  setDraft,
  sending,
  disabled,
  onSend,
  error,
  onDismissError,
  offline = false,
  recovery,
  commands,
  runActive = false,
  onStop,
  onSteer,
  attachments,
  conversationKey,
  savedAttachmentCount = 0,
  voiceDisabled = false,
  nativeVoice,
  tools,
}: {
  agentName: string;
  /** The textarea's name and placeholder; defaults to "Message <agent>". */
  label?: string;
  draft: string;
  setDraft: (v: string) => void;
  sending: boolean;
  disabled: boolean;
  /** Sends the draft, or `text` — a command picked from the menu. */
  onSend: (text?: string) => void;
  error: string | null;
  onDismissError: () => void;
  offline?: boolean;
  recovery?: {
    count: number;
    text: string;
    restore: () => void;
    dismiss: () => void;
  };
  /** The slash commands the menu offers (spec §15.3); none without them. */
  commands?: readonly SlashCommand[];
  /** This session's reply is in progress: Send becomes Stop and
   *  ⌘/Ctrl+Enter steers it (spec §15.3). */
  runActive?: boolean;
  onStop?: () => void;
  onSteer?: () => void;
  conversationKey?: string | null;
  voiceDisabled?: boolean;
  nativeVoice?: NativeVoiceScope;
  savedAttachmentCount?: number;
  /** The session's own input tools (browser task, spoken replies), kept
   *  with Attach and Record behind the composer's + button. */
  tools?: ReactNode;
  attachments?: {
    items: readonly AttachmentDraft[];
    onAdd(files: readonly File[]): void;
    onRemove(localId: string): void;
    error?: string | null;
  };
}) {
  const taRef = useRef<HTMLTextAreaElement>(null);
  const latestDraft = useRef(draft);
  latestDraft.current = draft;
  const changeDraft = (value: string) => {
    latestDraft.current = value;
    setDraft(value);
  };
  const appendDictation = (text: string) => {
    const previous = latestDraft.current.trimEnd();
    changeDraft(`${previous}${previous ? ' ' : ''}${text}`);
  };
  const menuId = useId();
  const trayId = useId();
  const [moreOpen, setMoreOpen] = useState(false);
  const moreButton = useRef<HTMLButtonElement>(null);
  const hasTray = Boolean(attachments || tools);
  const [activeIndex, setActiveIndex] = useState(0);
  const [dismissedFor, setDismissedFor] = useState<string | null>(null);
  const [blurred, setBlurred] = useState(false);
  const inputLabel = label ?? `Message ${agentName}`;
  const suggestions = commands ? slashSuggestions(draft, commands) : [];
  const menuOpen = suggestions.length > 0 && dismissedFor !== draft && !blurred;
  const selected = menuOpen
    ? suggestions[Math.min(activeIndex, suggestions.length - 1)]
    : null;
  const attachmentsBlocked =
    attachments?.items.some((item) => item.status !== 'ready') ?? false;
  const hasAttachments =
    (attachments?.items.some((item) => item.status === 'ready') ?? false) ||
    savedAttachmentCount > 0;
  const canSend =
    !disabled &&
    !sending &&
    !offline &&
    !attachmentsBlocked &&
    (draft.trim().length > 0 || hasAttachments);
  const canPick = !disabled && !sending && !offline;
  const steerable = runActive && onSteer !== undefined;
  // One button: the mic until there is something to send, then Send. While
  // dictation listens the mic stays, so its words never stop it.
  const listening = usePresenceSignals().listening !== null;
  const showSend =
    !listening &&
    (draft.trim().length > 0 ||
      (attachments?.items.length ?? 0) > 0 ||
      savedAttachmentCount > 0);

  useEffect(() => {
    setActiveIndex(0);
  }, [draft]);

  useEffect(() => {
    const el = taRef.current;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, 192)}px`;
  }, [draft]);

  /** Runs a command that needs nothing more; completes one that does, and
   *  a skill, which takes the request after its name (spec §15.3). */
  const pick = (command: SlashCommand, complete = false) => {
    const takesText = Boolean(command.needs || command.skill);
    if (takesText || complete) {
      setDraft(`/${command.name}${takesText ? ' ' : ''}`);
      taRef.current?.focus();
      return;
    }
    if (canPick) onSend(`/${command.name}`);
  };

  return (
    <div className="studio-composer safe-composer sticky bottom-0 z-10 bg-gradient-to-t from-abyss via-abyss/95 to-transparent px-4 pt-3 sm:px-6">
      <div className="mx-auto w-full max-w-3xl">
        {recovery && (
          <div className="studio-draft-recovery">
            <div>
              <p>
                {recovery.count} recoverable{' '}
                {recovery.count === 1 ? 'message' : 'messages'}. Check the
                conversation before retrying—it may have reached the daemon.
              </p>
              <blockquote>{recovery.text}</blockquote>
            </div>
            <div>
              <button
                type="button"
                className="studio-tool-button"
                onClick={() => {
                  recovery.restore();
                  taRef.current?.focus();
                }}
              >
                Restore message
              </button>
              <button
                type="button"
                className="studio-tool-button"
                onClick={recovery.dismiss}
                aria-label="Dismiss recoverable message"
              >
                ×
              </button>
            </div>
          </div>
        )}
        {error && (
          <div className="mb-2.5">
            <ErrorBanner
              message={error}
              onDismiss={onDismissError}
              icon={<AlertIcon size={14} />}
            />
          </div>
        )}
        {menuOpen && (
          <SlashCommandMenu
            id={menuId}
            commands={suggestions}
            activeName={selected?.name ?? null}
            onPick={(command) => pick(command)}
          />
        )}
        {attachments && (
          <AttachmentChips
            items={attachments.items}
            onRemove={attachments.onRemove}
          />
        )}
        {attachments?.error && (
          <p role="alert" className="text-xs text-danger">
            {attachments.error}
          </p>
        )}
        {hasTray && (
          // Closed, the tray shows only what is live: a recording, audio to
          // transcribe. Opened from +, it offers every extra input.
          <div
            id={trayId}
            className="composer-tray"
            data-open={moreOpen || undefined}
            onKeyDown={(event) => {
              if (event.key !== 'Escape' || event.defaultPrevented) return;
              event.preventDefault();
              setMoreOpen(false);
              moreButton.current?.focus();
            }}
          >
            {attachments && (
              <AttachmentControls
                disabled={!canPick}
                onAdd={attachments.onAdd}
              />
            )}
            {attachments && (
              <MicrophoneCapture
                key={conversationKey ?? ''}
                conversationKey={conversationKey ?? ''}
                disabled={!conversationKey || !canPick || voiceDisabled}
                onAdd={attachments.onAdd}
              />
            )}
            {nativeVoice && attachments && (
              <NativeTranscription
                key={JSON.stringify([
                  conversationKey,
                  nativeVoice.agentId,
                  nativeVoice.sessionId,
                ])}
                scope={nativeVoice}
                conversationKey={conversationKey}
                items={attachments.items}
                disabled={!canPick || voiceDisabled}
                onUse={appendDictation}
              />
            )}
            {tools}
          </div>
        )}
        <AttachmentDropZone disabled={!canPick} onAdd={attachments?.onAdd}>
          <div className="studio-composer-box composer-bar glass-strong focus-glow flex items-end gap-1 p-1.5 transition-all duration-200">
            {/* The agent's orb lives in the composer: it brightens while you
                write and while the agent works. */}
            <span className="composer-orb" aria-hidden>
              <HaloOrb
                energy={runActive || sending ? 0.8 : draft.trim() ? 0.6 : 0.35}
                className="composer-orb-canvas"
              />
            </span>
            <textarea
              data-workspace-composer
              ref={taRef}
              value={draft}
              disabled={disabled}
              onChange={(e) => changeDraft(e.target.value)}
              onKeyDown={(e) => {
                const composing =
                  e.nativeEvent.isComposing || e.keyCode === 229;
                if (selected) {
                  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
                    e.preventDefault();
                    const step = e.key === 'ArrowDown' ? 1 : -1;
                    setActiveIndex(
                      (index) =>
                        (Math.min(index, suggestions.length - 1) +
                          step +
                          suggestions.length) %
                        suggestions.length,
                    );
                    return;
                  }
                  if (e.key === 'Escape') {
                    e.preventDefault();
                    e.stopPropagation();
                    setDismissedFor(draft);
                    return;
                  }
                  if (
                    (e.key === 'Tab' && !e.shiftKey) ||
                    (e.key === 'Enter' && !e.shiftKey && !composing)
                  ) {
                    e.preventDefault();
                    pick(selected, e.key === 'Tab');
                    return;
                  }
                }
                if (e.key === 'Enter' && !e.shiftKey && !composing) {
                  e.preventDefault();
                  if (!canSend) return;
                  if ((e.metaKey || e.ctrlKey) && steerable) onSteer?.();
                  else onSend();
                }
              }}
              onFocus={() => setBlurred(false)}
              onBlur={() => setBlurred(true)}
              rows={1}
              aria-label={inputLabel}
              aria-autocomplete={commands ? 'list' : undefined}
              aria-controls={menuOpen ? menuId : undefined}
              aria-activedescendant={
                selected ? `${menuId}-${selected.name}` : undefined
              }
              placeholder={`${inputLabel}…`}
              className="max-h-48 flex-1 resize-none bg-transparent px-2 py-[7px] text-[15px] leading-6 text-ink placeholder-ink-3 outline-none"
            />
            {hasTray && (
              <button
                ref={moreButton}
                type="button"
                className="composer-icon composer-more"
                aria-label="More input options"
                aria-expanded={moreOpen}
                aria-controls={trayId}
                title="Attach files, record audio, and more"
                onClick={() => setMoreOpen((open) => !open)}
              >
                <PlusIcon size={16} />
              </button>
            )}
            {runActive && onStop ? (
              <button
                type="button"
                onClick={onStop}
                aria-label="Stop"
                className="composer-send flex shrink-0 cursor-pointer items-center justify-center transition active:scale-95"
              >
                <StopIcon size={14} />
              </button>
            ) : !showSend ? (
              <DictationControl
                compact
                disabled={!canPick || voiceDisabled}
                conversationKey={conversationKey}
                onTranscript={appendDictation}
              />
            ) : (
              <button
                type="button"
                onClick={() => onSend()}
                disabled={!canSend}
                aria-label="Send"
                className="composer-send flex shrink-0 cursor-pointer items-center justify-center transition active:scale-95 disabled:cursor-not-allowed disabled:active:scale-100"
              >
                <SendIcon size={14} />
              </button>
            )}
          </div>
        </AttachmentDropZone>
        {/* A hint only when there is something to know. */}
        {(steerable || offline || sending || runActive) && (
          <div className="composer-hints">
            {steerable && (
              <span>
                {/* Ctrl+Enter on every non-Mac platform, never ⌘⏎ (S3b-E). */}
                {`⏎ queue · ${isMacPlatform() ? '⌘⏎' : 'Ctrl+Enter'} steer · ⇧⏎ new line`}
              </span>
            )}
            <span>
              {offline
                ? 'Offline · your draft stays here'
                : sending
                  ? 'Working on your message…'
                  : 'Replying · you can keep writing'}
            </span>
          </div>
        )}
      </div>
    </div>
  );
}

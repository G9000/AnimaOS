import { useLatest } from '../hooks/useLatest';
import { useResetOnChange } from '../hooks/useResetOnChange';
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
import { CopyMessage } from './CopyMessage';
import { SaveToMemory } from './memory/SaveToMemory';
import {
  SecondOpinionAction,
  SecondOpinionNote,
} from './sessions/SecondOpinion';
import type { SecondOpinions } from '../hooks/useSecondOpinions';
import type { SaveOutcome, SavedState } from '../hooks/useSaveToMemory';
import type { ActionReceipt, SessionSummary } from '@animaos/sdk';
import type { AgentDetail, ChatMessage } from '../lib/types';
import {
  AlertIcon,
  BoltIcon,
  PulseIcon,
  SendIcon,
  StopIcon,
} from './icons';
import { MarkdownMessage } from './MarkdownMessage';
import { ErrorBanner, formatTime } from './ui-bits';
import { isMacPlatform } from '../lib/platform';
import {
  ComposerMode,
  type ComposerModeState,
  type ComposerModelState,
} from './ComposerMode';
import { ComposerAdd } from './ComposerAdd';
import {
  buildTranscript,
  isAcceptedBudgetWrapupMessage,
  type TranscriptActions,
  type TranscriptItem,
} from '../lib/transcript';
import { PendingMessage, RunActivity, ToolBlock } from './sessions/RunActivity';
import { RunOutcomeCard } from './sessions/RunOutcomeCard';
import { AutomationMessageCard } from './sessions/AutomationMessageCard';
import { TaskReviewAction } from './learning/TaskReviewAction';
import { TaskReviewProvider } from './learning/TaskReviewContext';
import {
  DelegatedTurn,
  KeptSummary,
  TrimmedDivider,
} from './sessions/TranscriptNotes';
import { slashSuggestions, type SlashCommand } from '../lib/slash-commands';
import {
  loadSent,
  pushSent,
  recallNewer,
  recallOlder,
  type Recall,
} from '../lib/sent-history';
import { SlashCommandMenu } from './sessions/SlashCommandMenu';
import { toolLabel } from '../lib/tool-labels';
import {
  AttachmentDropZone,
} from './attachments/AttachmentControls';
import { AttachmentChips } from './attachments/AttachmentChips';
import { MessageAttachments } from './attachments/MessageAttachments';
import type { AttachmentDraft } from '../lib/attachments';
import { DictationControl } from './voice/DictationControl';
import type { NativeVoiceScope } from './voice/NativeTranscription';
import { ReadAloud } from './voice/ReadAloud';
import type { ReadAloudController } from '../hooks/useReadAloud';
import { revealInvisible } from '../lib/skills';
import {
  consumeRecentSpokenInput,
  markSpokenInput,
  useVoiceSettings,
} from '../lib/voice-settings';
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

/** The automation behind a post it made in a channel's own thread (the
 *  daemon's copy of a Discord post): an ordinary reply with a small tag. */
function automationPostName(message: ChatMessage): string | null {
  const metadata = message.content.metadata;
  if (message.role !== 'Assistant' || metadata?.kind !== 'automationPost')
    return null;
  const name = metadata.automationName;
  return typeof name === 'string' && name.trim() ? name : 'Automation';
}

/** Where an owner message came from, when not this console: the owner's
 *  private Telegram chat shares the main chat. */
const SOURCE_LABELS: Record<string, string> = {
  telegram: 'Telegram',
  discord: 'Discord',
  email: 'Email',
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
  secondOpinions,
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
  /** Offered on the companion's saved replies. */
  secondOpinions?: SecondOpinions;
}) {
  if (message.role !== 'User' && message.role !== 'Assistant') {
    return <EventPill message={message} />;
  }
  const isUser = message.role === 'User';
  const flag = messageFlag(message);
  const time = formatTime(message.created_at_ms);
  const source = messageSource(message);
  const automationPost = automationPostName(message);
  const review =
    !isUser && secondOpinions ? secondOpinions.reviewFor(message.id) : null;
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
          {automationPost && (
            <span className="chat-source">
              Automation · {revealInvisible(automationPost).text}
            </span>
          )}
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
      <div className="chat-actions studio-message-meta">
        {source && <span className="chat-source">via {source}</span>}
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
        {!isUser && secondOpinions && message.content.text.trim() !== '' && (
          <SecondOpinionAction
            messageId={message.id}
            review={review}
            busy={secondOpinions.busy}
            onRequest={secondOpinions.request}
          />
        )}
      </div>
      {review && (
        <SecondOpinionNote review={review} onCancel={secondOpinions!.cancel} />
      )}
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
    case 'automation':
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
    item.kind === 'automation' ||
    item.kind === 'delegated' ||
    item.kind === 'revised'
    ? item.message.created_at_ms
    : item.kind === 'receipts'
      ? item.createdAtMs
    : null;
}

function ActionReceiptCard({
  receipts,
  createdAtMs,
}: {
  receipts: ActionReceipt[];
  createdAtMs: number;
}) {
  const [expanded, setExpanded] = useState(false);
  if (!receipts.length) return null;
  const successes = receipts.filter(
    (receipt) => receipt.status === 'succeeded',
  ).length;
  const failures = receipts.length - successes;
  const visibleReceipts = expanded ? receipts : receipts.slice(0, 4);
  return (
    <section className="action-receipts-card" aria-label="What I did">
      <header className="action-receipts-head">
        <span>WHAT I DID</span>
        <span>
          {receipts.length} {receipts.length === 1 ? 'ACTION' : 'ACTIONS'}
          {successes > 0 && ` · ${successes} REPORTED SUCCESS`}
          {failures > 0 && ` · ${failures} REPORTED FAILURE`}
        </span>
        {receipts.length > 4 && (
          <button
            type="button"
            className="action-receipts-toggle"
            aria-expanded={expanded}
            onClick={() => setExpanded((value) => !value)}
          >
            {expanded ? 'Show fewer' : `Show all ${receipts.length}`}
          </button>
        )}
      </header>
      <ul className="action-receipts">
        {visibleReceipts.map((receipt) => {
          const status =
            receipt.status === 'succeeded'
              ? 'success'
              : receipt.status === 'failed'
                ? 'failed'
                : 'unknown';
          const accessibleStatus =
            status === 'success'
              ? 'Reported success'
              : status === 'failed'
                ? 'Reported failure'
                : 'Outcome unknown';
          return (
            <li key={receipt.toolCallId} data-status={status}>
              <span className="action-receipt-status" aria-hidden>
                {status === 'success' ? '✓' : status === 'failed' ? '!' : '·'}
              </span>
              <span className="action-receipt-label">
                {toolLabel(receipt.name)}
              </span>
              <time>{formatTime(receipt.finishedAtMs ?? createdAtMs)}</time>
              <span className="sr-only">{accessibleStatus}</span>
            </li>
          );
        })}
      </ul>
    </section>
  );
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

/** Short names for the day rail: Today, a weekday this week, then "Last wk"
 *  and months. */
function railLabel(ms: number, now = new Date()): string {
  const day = new Date(ms);
  const startOf = (d: Date) =>
    new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const diff = Math.round((startOf(now) - startOf(day)) / 86_400_000);
  if (diff === 0) return 'Today';
  if (diff < 7) return day.toLocaleDateString([], { weekday: 'short' });
  if (diff < 14) return 'Last wk';
  return day.toLocaleDateString([], { month: 'short' });
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
  turnTime,
}: {
  item: TranscriptItem;
  agentName: string;
  /** When the reply this tool work leads into was sent. */
  turnTime?: number | null;
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
            secondOpinions={actions?.secondOpinions}
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
    case 'automation':
      return (
        <AutomationMessageCard
          meta={item.meta}
          actions={actions?.automationCards}
        />
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
      return (
        <ToolBlock
          steps={item.steps}
          active={false}
          actions={actions}
          head={{
            name: agentName,
            time: turnTime == null ? undefined : formatTime(turnTime),
          }}
        />
      );
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
    case 'receipts':
      return (
        <ActionReceiptCard
          receipts={item.receipts}
          createdAtMs={item.createdAtMs}
        />
      );
    case 'learning-review':
      return <TaskReviewAction run={item.run} />;
    case 'model-note':
      return <p className="model-note">{item.text}</p>;
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
  loading = false,
  emptyState,
  tail,
  items,
  actions,
  readAloud,
  conversationKey,
  knownAttachments = EMPTY_RUN_ATTACHMENTS,
  highlightMessageId = null,
  jumpRequestKey = 0,
  jumpReady = true,
  onJumpToLatest,
  onAtBottomChange,
  summary,
  summaryKey,
}: {
  agent: AgentDetail;
  sending: boolean;
  scrollerRef: React.RefObject<HTMLDivElement | null>;
  onSuggestion: (text: string) => void;
  /** Older history exists: show a control and load it on scroll to the top. */
  hasOlder?: boolean;
  loadingOlder?: boolean;
  onLoadOlder?: () => void;
  /** The first page of history is still on its way: show nothing rather
   *  than the welcome, which would flash before the messages arrive. */
  loading?: boolean;
  /** Replaces the welcome screen for sessions that are not new chats. */
  emptyState?: ReactNode;
  /** Agent-scoped controls rendered after the transcript inside its scroller. */
  tail?: ReactNode;
  /** The session's transcript with its live runs and sends (spec §15.2);
   *  built from `agent.messages` when absent. */
  items?: readonly TranscriptItem[];
  actions?: TranscriptActions;
  readAloud?: ReadAloudController;
  conversationKey?: string | null;
  knownAttachments?: KnownRunAttachments;
  /** A bounded history result; waits for its transcript anchor to mount. */
  highlightMessageId?: string | null;
  jumpRequestKey?: number;
  /** The requested historical window has finished loading successfully. */
  jumpReady?: boolean;
  onJumpToLatest?: () => void;
  /** Tells the page whether the newest message is in view, so a toast for a
   *  new automation card shows only when it would otherwise be missed. */
  onAtBottomChange?: (atBottom: boolean) => void;
  /** Persisted summary shown as the first card in the main chat. */
  summary?: SessionSummary;
  /** Remounts the summary disclosure when the active conversation changes. */
  summaryKey?: string;
}) {
  const [awayFromBottom, setAwayFromBottom] = useState(false);
  useEffect(() => {
    onAtBottomChange?.(!awayFromBottom);
  }, [awayFromBottom, onAtBottomChange]);
  const highlightedMessageId = jumpReady ? highlightMessageId : null;
  const atBottom = useRef(true);
  const tailRef = useRef<HTMLDivElement>(null);
  const hasTail = tail != null;
  useLayoutEffect(() => {
    const element = tailRef.current;
    if (!element || typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(() => {
      const scroller = scrollerRef.current;
      if (atBottom.current && scroller)
        scroller.scrollTop = scroller.scrollHeight;
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [hasTail, scrollerRef]);
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
  const voiceSettings = useVoiceSettings();
  const latestAssistant = useMemo(() => {
    for (let index = transcript.length - 1; index >= 0; index--) {
      const item = transcript[index];
      if (item.kind === 'message' && item.message.role === 'Assistant')
        return item.message;
    }
    return null;
  }, [transcript]);
  const lastAutoRead = useRef<{
    conversationKey: string | null | undefined;
    messageId: string | null;
  }>({ conversationKey, messageId: latestAssistant?.id ?? null });
  useEffect(() => {
    if (lastAutoRead.current.conversationKey !== conversationKey) {
      lastAutoRead.current = {
        conversationKey,
        messageId: latestAssistant?.id ?? null,
      };
      return;
    }
    if (!latestAssistant || lastAutoRead.current.messageId === latestAssistant.id)
      return;
    lastAutoRead.current.messageId = latestAssistant.id;
    if (
      !voiceSettings.readRepliesAfterSpeech ||
      !readAloud?.supported ||
      readAloud.disabled ||
      !consumeRecentSpokenInput(conversationKey)
    )
      return;
    readAloud.read(latestAssistant.id, latestAssistant.content.text);
  }, [
    latestAssistant,
    conversationKey,
    readAloud,
    voiceSettings.readRepliesAfterSpeech,
  ]);
  const anchorsByItem = useMemo(() => anchorsFor(transcript), [transcript]);
  const showsReviews = useMemo(
    () => transcript.some((item) => item.kind === 'learning-review'),
    [transcript],
  );
  const days = useMemo(() => dayDividers(transcript), [transcript]);
  const dayKeys = useMemo(
    () =>
      transcript.map((item) => {
        const ms = itemTime(item);
        return ms === null ? null : new Date(ms).toDateString();
      }),
    [transcript],
  );
  const dayNames = useMemo(() => {
    const names = new Map<string, string>();
    transcript.forEach((item, index) => {
      const key = dayKeys[index];
      const ms = itemTime(item);
      if (key && ms !== null && !names.has(key)) names.set(key, dayLabel(ms));
    });
    return names;
  }, [transcript, dayKeys]);
  // The day at the top of the screen follows the scroll; before any scroll it
  // is the newest day, since the chat opens at the bottom.
  const [topDay, setTopDay] = useState<string | null>(null);
  const newestDay = useMemo(() => {
    for (let index = dayKeys.length - 1; index >= 0; index--) {
      const key = dayKeys[index];
      if (key) return key;
    }
    return null;
  }, [dayKeys]);
  const currentDay =
    topDay !== null && dayNames.has(topDay) ? topDay : newestDay;
  const trackFrame = useRef<number | null>(null);
  const trackTopDay = useCallback(() => {
    const scroller = scrollerRef.current;
    if (!scroller) return;
    const top = scroller.getBoundingClientRect().top;
    let found: string | null = null;
    for (const element of scroller.querySelectorAll<HTMLElement>(
      '[data-day]',
    )) {
      if (element.getBoundingClientRect().bottom > top + 4) {
        found = element.dataset.day ?? null;
        break;
      }
    }
    setTopDay(found);
  }, [scrollerRef]);
  const scheduleTrackTopDay = useCallback(() => {
    if (typeof requestAnimationFrame !== 'function') {
      trackTopDay();
      return;
    }
    if (trackFrame.current !== null) return;
    trackFrame.current = requestAnimationFrame(() => {
      trackFrame.current = null;
      trackTopDay();
    });
  }, [trackTopDay]);
  useEffect(
    () => () => {
      if (trackFrame.current !== null) cancelAnimationFrame(trackFrame.current);
    },
    [],
  );
  /** Where each loaded day begins, for the rail beside the transcript. */
  const dayStops = useMemo(() => {
    const stops: { key: string; label: string; id: string }[] = [];
    let previous: string | null = null;
    transcript.forEach((item, index) => {
      const ms = itemTime(item);
      const id = anchorsByItem[index]?.[0];
      if (ms === null || !id) return;
      const day = new Date(ms).toDateString();
      if (day === previous) return;
      previous = day;
      stops.push({ key: day, label: railLabel(ms), id });
    });
    return stops;
  }, [transcript, anchorsByItem]);
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
  const [previousHighlight, setPreviousHighlight] =
    useState(highlightMessageId);
  useResetOnChange(
    [
      highlightMessageId,
      highlightMessageId ? jumpRequestKey : null,
      highlightMessageId ? jumpReady : null,
      agent.id,
      highlightMessageId ? transcript : null,
      highlightMessageId ? firstMessageId : null,
      highlightMessageId ? scrollerRef : null,
    ],
    () => {
      if (highlightMessageId) setAwayFromBottom(true);
      else if (previousHighlight) setAwayFromBottom(false);
      setPreviousHighlight(highlightMessageId);
    },
  );
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
      if (!highlightMessageId && previous.messageId) {
        atBottom.current = true;
        const scroller = scrollerRef.current;
        if (scroller) scroller.scrollTop = scroller.scrollHeight;
      }
    }
    if (!highlightMessageId) return;
    // Disable bottom-follow even while the requested window is loading.
    atBottom.current = false;
    if (!jumpReady || externalJump.current.done) return;
    const element = messageElements.current.get(highlightMessageId);
    if (!element) return;
    externalJump.current.done = true;
    element.scrollIntoView?.({ block: 'center' });
    // The ordinary prepend-preservation effect below must not offset the
    // position we just chose when the anchor window replaced the page.
    anchor.current = {
      firstId: firstMessageId,
      height:
        (scrollerRef.current?.scrollHeight ?? 0) -
        (tailRef.current?.offsetHeight ?? 0),
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
    // Workflow cards resize independently of history. Only transcript growth
    // above the reading position belongs in the prepend adjustment.
    const height = element.scrollHeight - (tailRef.current?.offsetHeight ?? 0);
    const previous = anchor.current;
    if (
      previous.firstId !== undefined &&
      previous.firstId !== firstMessageId &&
      !atBottom.current
    ) {
      element.scrollTop += height - previous.height;
    }
    anchor.current = { firstId: firstMessageId, height };
  }, [firstMessageId, agent.messages, scrollerRef]);
  // After the effects above settle the scroll position.
  useLayoutEffect(() => {
    trackTopDay();
  }, [transcript, trackTopDay]);

  return (
    <>
      <div className="studio-conversation-body">
        {dayStops.length > 1 && (
          <nav className="day-rail" aria-label="Jump to a day">
            {dayStops.map((stop, index) => (
              <button
                key={stop.key}
                type="button"
                className="day-rail-stop"
                data-current={stop.key === currentDay || undefined}
                onClick={() => jumpToMessage(stop.id)}
              >
                {stop.label}
                <i aria-hidden />
              </button>
            ))}
          </nav>
        )}
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
            scheduleTrackTopDay();
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
          {transcript.length === 0 && !sending && loading ? (
            <div className="studio-transcript-loading" aria-busy="true">
              <span className="sr-only">Loading conversation…</span>
            </div>
          ) : transcript.length === 0 && !sending ? (
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
                {summary && (
                  <KeptSummary
                    key={summaryKey ?? summary.createdAtMs}
                    summary={summary}
                    onShowOriginals={hasOlder ? onLoadOlder : undefined}
                  />
                )}
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
                      data-day={dayKeys[index] ?? undefined}
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
                        turnTime={
                          item.kind === 'tools'
                            ? itemTime(transcript[index + 1] ?? item)
                            : undefined
                        }
                      />
                    </div>
                  );
                })}
                {sending && <ThinkingIndicator name={agent.name} />}
              </div>
            </TaskReviewProvider>
          )}
          {tail != null && (
            <div
              ref={tailRef}
              className="mx-auto w-full max-w-3xl px-4 pb-6 sm:px-6"
            >
              {tail}
            </div>
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
/** Set when a focused composer is replaced (the chat settling on its main
 *  session, a loading state): the next composer takes focus back so the
 *  owner's typing continues instead of landing on the page. */
let composerFocusHandoff = false;

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
  tools,
  context,
  mode,
  model,
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
    /** False while the chat cannot take the message back yet (loading,
     *  offline): Restore waits instead of silently doing nothing. */
    ready?: boolean;
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
  /** Additional session actions shown beside the composer. */
  tools?: ReactNode;
  /** Current quoted selection, kept visible beside the editable draft. */
  context?: ReactNode;
  /** Plan or Do for what is sent next; with `model`, the chip beside the
   *  message. */
  mode?: ComposerModeState;
  model?: ComposerModelState;
  attachments?: {
    items: readonly AttachmentDraft[];
    onAdd(files: readonly File[]): void;
    onRemove(localId: string): void;
    error?: string | null;
  };
}) {
  const taRef = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    const textarea = taRef.current;
    if (
      composerFocusHandoff &&
      textarea &&
      (document.activeElement === null ||
        document.activeElement === document.body)
    ) {
      textarea.focus();
      const end = textarea.value.length;
      textarea.setSelectionRange(end, end);
    }
    composerFocusHandoff = false;
    return () => {
      composerFocusHandoff =
        textarea !== null && document.activeElement === textarea;
    };
  }, []);
  const latestDraftRef = useLatest(draft);
  const spokenDraftRef = useRef(false);
  const historyKey = conversationKey ?? agentName;
  const recallRef = useRef<Recall>({ cursor: -1, stash: '' });
  const changeDraft = (value: string) => {
    latestDraftRef.current = value;
    setDraft(value);
  };
  /** Shows a recalled message with the caret after its last character. */
  const showRecalled = (value: string) => {
    changeDraft(value);
    requestAnimationFrame(() =>
      taRef.current?.setSelectionRange(value.length, value.length),
    );
  };
  const appendDictation = (text: string) => {
    spokenDraftRef.current = true;
    const previous = latestDraftRef.current.trimEnd();
    changeDraft(`${previous}${previous ? ' ' : ''}${text}`);
  };
  const send = (text?: string) => {
    if (spokenDraftRef.current) {
      markSpokenInput(conversationKey);
      spokenDraftRef.current = false;
    }
    pushSent(historyKey, text ?? latestDraftRef.current);
    recallRef.current = { cursor: -1, stash: '' };
    onSend(text);
  };
  const menuId = useId();
  // Words dictation is still hearing, shown after the draft until they
  // settle into it.
  const [heard, setHeard] = useState('');
  const shownDraft = heard
    ? `${draft.trimEnd()}${draft.trim() ? ' ' : ''}${heard}`
    : draft;
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

  useResetOnChange([draft], () => setActiveIndex(0));
  useResetOnChange([historyKey], () => {
    recallRef.current = { cursor: -1, stash: '' };
  });

  // Fit the field to its text. It measures again when its width changes:
  // measured before layout, at no width, the empty field filled its
  // 192px cap.
  useEffect(() => {
    const el = taRef.current;
    if (!el) return;
    const fit = () => {
      el.style.height = 'auto';
      el.style.height = `${Math.min(el.scrollHeight, 192)}px`;
    };
    fit();
    if (typeof ResizeObserver === 'undefined') return;
    let width = el.clientWidth;
    const observer = new ResizeObserver(() => {
      if (el.clientWidth === width) return;
      width = el.clientWidth;
      fit();
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [shownDraft]);

  /** Runs a command that needs nothing more; completes one that does, and
   *  a skill, which takes the request after its name (spec §15.3). */
  const pick = (command: SlashCommand, complete = false) => {
    const takesText = Boolean(command.needs || command.skill || command.turn);
    if (takesText || complete) {
      setDraft(`/${command.name}${takesText ? ' ' : ''}`);
      taRef.current?.focus();
      return;
    }
    if (canPick) send(`/${command.name}`);
  };

  return (
    <div className="studio-composer safe-composer sticky bottom-0 z-10 bg-gradient-to-t from-abyss via-abyss/95 to-transparent px-4 pt-3 sm:px-6">
      <div className="composer-inner mx-auto w-full max-w-3xl">
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
                disabled={recovery.ready === false}
                title={
                  recovery.ready === false
                    ? 'Available once this chat has loaded'
                    : undefined
                }
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
        {context}
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
        {tools}
        <AttachmentDropZone disabled={!canPick} onAdd={attachments?.onAdd}>
          <div className="studio-composer-box composer-bar glass-strong flex flex-col p-1.5 transition-[background-color,border-color,box-shadow] duration-200">
            <div className="composer-row">
              <ComposerAdd
                attachments={
                  attachments && { disabled: !canPick, onAdd: attachments.onAdd }
                }
                commands={
                  commands && {
                    disabled: disabled || draft.trim() !== '',
                    onStart: () => {
                      changeDraft('/');
                      taRef.current?.focus();
                    },
                  }
                }
              />
              <textarea
                data-workspace-composer
                ref={taRef}
                value={shownDraft}
                // Held still while words are being heard, so typing never
                // races the transcript.
                readOnly={heard !== ''}
                disabled={disabled}
                onChange={(e) => {
                  // Typing makes a recalled message the user's own draft.
                  recallRef.current = { cursor: -1, stash: '' };
                  changeDraft(e.target.value);
                }}
                onKeyDown={(e) => {
                  const composing =
                    e.nativeEvent.isComposing || e.keyCode === 229;
                  // ↑ on the first line brings back the last message sent
                  // here, ↓ on the last line walks forward to the draft.
                  if (
                    !selected &&
                    !composing &&
                    !disabled &&
                    (e.key === 'ArrowUp' || e.key === 'ArrowDown') &&
                    !e.shiftKey &&
                    !e.altKey &&
                    !e.ctrlKey &&
                    !e.metaKey
                  ) {
                    const field = e.currentTarget;
                    const up = e.key === 'ArrowUp';
                    const onEdge = up
                      ? !field.value.slice(0, field.selectionStart).includes('\n')
                      : !field.value.slice(field.selectionEnd).includes('\n');
                    const free =
                      recallRef.current.cursor !== -1 || draft.length === 0;
                    if (onEdge && (up ? free : recallRef.current.cursor !== -1)) {
                      const items = loadSent(historyKey);
                      const step = up
                        ? recallOlder(items, recallRef.current, draft)
                        : recallNewer(items, recallRef.current);
                      if (step) {
                        e.preventDefault();
                        recallRef.current = step.recall;
                        showRecalled(step.draft);
                        return;
                      }
                    }
                  }
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
                    else send();
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
              <div className="composer-controls">
                <ComposerMode mode={mode} model={model} />
                {runActive && (
                  <>
                    <button
                      type="button"
                      className="composer-queue-action cursor-pointer rounded px-2 py-1 text-xs text-ink-2 disabled:opacity-40"
                      disabled={!canSend}
                      onClick={() => send()}
                      title="Wait for the current reply, then send this message"
                    >
                      Wait
                    </button>
                    {steerable && (
                      <button
                        type="button"
                        className="composer-queue-action cursor-pointer rounded px-2 py-1 text-xs text-ink-2 disabled:opacity-40"
                        disabled={!canSend}
                        onClick={() => onSteer?.()}
                        title="Add this message to the reply in progress"
                      >
                        Steer
                      </button>
                    )}
                  </>
                )}
                <DictationControl
                  compact
                  disabled={!canPick || voiceDisabled}
                  conversationKey={conversationKey}
                  onTranscript={appendDictation}
                  onInterim={setHeard}
                  onPushToTalkStop={() => send(latestDraftRef.current)}
                />
                {runActive && onStop ? (
                  <button
                    type="button"
                    onClick={onStop}
                    aria-label="Stop"
                    className="composer-send composer-stop flex shrink-0 cursor-pointer items-center justify-center transition active:scale-95"
                  >
                    <StopIcon size={14} />
                  </button>
                ) : (
                  <button
                    type="button"
                    onClick={() => send()}
                    disabled={!canSend}
                    aria-label="Send"
                    className="composer-send flex shrink-0 cursor-pointer items-center justify-center transition active:scale-95 disabled:cursor-not-allowed disabled:active:scale-100"
                  >
                    <SendIcon size={14} />
                  </button>
                )}
              </div>
            </div>
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

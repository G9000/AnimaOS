import {
  useEffect,
  useMemo,
  useState,
  type ReactNode,
  type RefObject,
} from 'react';
import {
  isTerminalRunStatus,
  type Automation,
  type Session,
  type BrowserSelection,
  type UploadedAttachment,
} from '@animaos/sdk';

import { useSaveToMemory } from '../../hooks/useSaveToMemory';
import { setPresenceSpeaking } from '../../lib/presence';
import { useSessionDetails } from '../../hooks/useSessionUsage';
import { describeTrigger } from '../../lib/automations';
import type { LiveRun } from '../../lib/session-events';
import { SESSION_KIND_LABELS } from '../../lib/session-groups';
import { revealInvisible } from '../../lib/skills';
import type { SlashCommand } from '../../lib/slash-commands';
import { sessionUsageLine } from '../../lib/usage';
import type { AgentDetail, ChatMessage } from '../../lib/types';
import { Composer, MessageList } from '../ChatScreen';
import type { ComposerModeState, ComposerModelState } from '../ComposerMode';
import {
  buildHistory,
  placeRuns,
  type PendingBubble,
  type TranscriptActions,
} from '../../lib/transcript';
import { ghostBtnCls } from '../ui-bits';
import { useReadAloud } from '../../hooks/useReadAloud';
import type { AttachmentDraft } from '../../lib/attachments';
import type { KnownRunAttachments } from '../../lib/run-attachments';
import { BrowserTaskControls } from './BrowserTaskControls';
import type { NativeVoiceScope } from '../voice/NativeTranscription';
import { NativeSpeech } from '../voice/NativeSpeech';
import { SessionSkillDrafts } from '../learning/SessionSkillDrafts';
import { ContextNote, SessionContinuity } from './SessionContinuity';
import { useSessionInteractions } from '../../hooks/useSessionInteractions';
import { ClarificationCard, SideQuestionCard } from './InteractionCards';
import { daemon } from '../../lib/daemon-api';
import { ChatWorkflows } from './ChatWorkflows';
import {
  ConversationControls,
  type ConversationControlProps,
} from './ConversationControls';
import { ChatExportScope } from './ChatExportDownload';
import { ChatExtensions, type ChatPanelRequest, type ChatPanelName } from './ChatExtensions';
import { ChatResultsLayout } from '../results/ChatResultsPanel';
import type { ChatModelPreferencesView } from '@animaos/sdk';

export interface SessionComposerState {
  draft: string;
  literalDraft?: boolean;
  setDraft: (value: string) => void;
  sending: boolean;
  disabled: boolean;
  offline: boolean;
  onSend: (text?: string) => void;
  error: string | null;
  onDismissError: () => void;
  recovery?: {
    count: number;
    text: string;
    restore: () => void;
    dismiss: () => void;
  };
  commands?: readonly SlashCommand[];
  /** This session's reply is in progress (spec §15.3). */
  runActive?: boolean;
  onStop?: () => void;
  onSteer?: () => void;
  conversationKey?: string | null;
  voiceDisabled?: boolean;
  nativeVoice?: NativeVoiceScope;
  savedAttachmentCount?: number;
  attachments?: {
    items: readonly AttachmentDraft[];
    onAdd(files: readonly File[]): void;
    onRemove(localId: string): void;
    error?: string | null;
  };
  /** Plan or Do for what is sent next. */
  mode?: ComposerModeState;
  /** The model the companion thinks with, and a way to change it. */
  model?: ComposerModelState;
}

export interface SessionViewProps {
  agent: AgentDetail;
  /** null for a new chat that has no session yet. */
  session: Session | null;
  messages: ChatMessage[];
  knownAttachments?: KnownRunAttachments;
  /** Messages on their way to the daemon (spec §15.5). */
  pending?: readonly PendingBubble[];
  /** The session's runs from its stream and ledger (spec §15.2). */
  runs?: readonly LiveRun[];
  actions?: TranscriptActions;
  /** Set for helper sessions: who wrote their user turns. */
  delegatedBy?: string | null;
  /** Read politely to screen readers when a reply finishes (spec §15.5). */
  announcement?: string;
  hasOlder: boolean;
  loadingOlder: boolean;
  onLoadOlder: () => void;
  /** The session was deleted elsewhere. */
  missing: boolean;
  telegramAvailable: boolean;
  discordAvailable?: boolean;
  scrollerRef: RefObject<HTMLDivElement | null>;
  onSuggestion: (text: string) => void;
  composer: SessionComposerState;
  onNewChat: () => void;
  onOpenWork: () => void;
  onRename: (title: string) => Promise<boolean>;
  onToggleArchived: () => void;
  onExport: () => void;
  /** A check-in's automation, for its header (spec §15.2). */
  automation?: Automation | null;
  /** Opens the automation's editor on the Automations page. */
  onEditAutomation?: (automation: Automation) => void;
  notice?: ReactNode;
  browserTask?: {
    selection: BrowserSelection | null;
    attachments: readonly UploadedAttachment[];
    onSelection(selection: BrowserSelection | null): void;
  };
  /** Skill drafts learned in this chat, offered under its transcript. */
  skillDrafts?: {
    version: number;
    epoch: number;
    enabled: boolean;
    onReview?: () => void;
  };
  /** Root-owned bounded history window for a saved message search result. */
  historyAnchor?: {
    messageId: string;
    loading: boolean;
    error: string | null;
    onReturnToLatest(): void;
  };
  jumpRequestKey?: number;
  /** The transcript's newest message is in view (or not). */
  onAtBottomChange?: (atBottom: boolean) => void;
  /** Activity outside a historical window that refreshes its context estimate. */
  contextRefreshKey?: number | string;
  extensions?: { request?: ChatPanelRequest | null; onOpen(panel: ChatPanelName): void; onModelSaved?(view: ChatModelPreferencesView): void };
  controls?: Pick<
    ConversationControlProps,
    'onRevise' | 'onQueuePatch' | 'onCancel' | 'onOpenSource' | 'revising'
  >;
}

export type SessionFooter =
  | { kind: 'composer'; label?: string }
  | { kind: 'note'; text: string; action?: 'work' };

/** What replaces the composer for each kind (spec §3.2, §15.2). */
export function sessionFooter(
  session: Session | null,
  telegramAvailable: boolean,
  discordAvailable = false,
): SessionFooter {
  if (!session) return { kind: 'composer' };
  switch (session.kind) {
    case 'chat':
      return { kind: 'composer' };
    case 'telegram':
      return telegramAvailable
        ? { kind: 'composer', label: 'Reply on Telegram' }
        : {
            kind: 'note',
            text: 'This Telegram connection is not available. Reconnect it in Connectors to reply.',
          };
    case 'checkin':
      return { kind: 'composer', label: 'Reply to this check-in' };
    case 'discord':
      return discordAvailable
        ? { kind: 'composer', label: 'Reply on Discord' }
        : {
            kind: 'note',
            text: 'This Discord connection cannot deliver right now. Check its status in Connectors before replying.',
          };
    case 'job':
      return {
        kind: 'note',
        text: 'Job sessions are read-only. Follow the job in Work.',
        action: 'work',
      };
    case 'email':
      return {
        kind: 'note',
        text: "Review and send email drafts in Connectors. Automatic replies follow this mailbox's sender policy.",
      };
    case 'helper':
      return { kind: 'note', text: 'Helper sessions are read-only.' };
  }
}

function SessionHeader({
  session,
  onRename,
  onToggleArchived,
  onExport,
  automation,
  onEditAutomation,
  usageLine,
}: {
  session: Session;
  onRename: (title: string) => Promise<boolean>;
  onToggleArchived: () => void;
  onExport: () => void;
  automation: Automation | null;
  onEditAutomation?: (automation: Automation) => void;
  /** Tokens and cost so far, e.g. '12.3k tokens · $0.04'. */
  usageLine: string | null;
}) {
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState(session.title);
  useEffect(() => {
    if (!editing) setTitle(session.title);
  }, [editing, session.title]);
  const cancel = () => {
    setEditing(false);
    setTitle(session.title);
  };

  return (
    <header className="session-view-header">
      {editing ? (
        <form
          className="flex min-w-0 flex-1 items-center gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            void onRename(title).then((saved) => {
              if (saved) setEditing(false);
            });
          }}
        >
          <input
            className="session-rename"
            aria-label="Session title"
            value={title}
            maxLength={120}
            autoFocus
            onChange={(event) => setTitle(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Escape') cancel();
            }}
          />
          <button type="submit" className={ghostBtnCls}>
            Save
          </button>
          <button type="button" className={ghostBtnCls} onClick={cancel}>
            Cancel
          </button>
        </form>
      ) : (
        <h2 className="session-view-title min-w-0 flex-1 truncate text-sm font-semibold text-ink">
          {session.title}
        </h2>
      )}
      <span className="session-kind-badge">
        {SESSION_KIND_LABELS[session.kind]}
        {automation
          ? ` · ${revealInvisible(describeTrigger(automation.trigger)).text}`
          : ''}
      </span>
      {usageLine && <span className="session-usage-line">{usageLine}</span>}
      {!editing && session.capabilities.rename && (
        <button
          type="button"
          className={ghostBtnCls}
          onClick={() => setEditing(true)}
        >
          Rename
        </button>
      )}
      {session.capabilities.archive && (
        <button
          type="button"
          className={ghostBtnCls}
          onClick={onToggleArchived}
        >
          {session.archived ? 'Unarchive' : 'Archive'}
        </button>
      )}
      {session.capabilities.export && (
        <button type="button" className={ghostBtnCls} onClick={onExport}>
          Export
        </button>
      )}
      {automation && onEditAutomation && (
        <button
          type="button"
          className={ghostBtnCls}
          onClick={() => onEditAutomation(automation)}
        >
          Edit automation
        </button>
      )}
    </header>
  );
}

const EMPTY_PENDING: readonly PendingBubble[] = [];
const EMPTY_RUNS: readonly LiveRun[] = [];
// One element, so a render of the view keeps the message list's props.
const NO_MESSAGES = <p className="session-footer-note">No messages yet.</p>;

/** The name saved with a memory when the agent's own name is not at hand. */
const GENERIC_SAVE_AGENT_NAME = 'Helper agent';

/** One session (or a new chat): its transcript and composer (spec §15.2). */
export function SessionView({
  agent,
  session,
  messages,
  knownAttachments,
  pending = EMPTY_PENDING,
  runs = EMPTY_RUNS,
  actions,
  delegatedBy = null,
  announcement = '',
  hasOlder,
  loadingOlder,
  onLoadOlder,
  missing,
  telegramAvailable,
  discordAvailable = false,
  scrollerRef,
  onSuggestion,
  composer,
  onNewChat,
  onOpenWork,
  onRename,
  onToggleArchived,
  onExport,
  automation = null,
  onEditAutomation,
  notice = null,
  browserTask,
  skillDrafts,
  historyAnchor,
  jumpRequestKey,
  onAtBottomChange,
  contextRefreshKey,
  controls,
  extensions,
}: SessionViewProps) {
  // The session record names its agent, so a helper's session saves to that
  // helper even when the agents list does not hold it (then `agent` is the
  // companion); the name is the agent's only when the ids match.
  const saveAgentId = session?.agentId ?? null;
  const saveAgentName =
    saveAgentId === agent.id ? agent.name : GENERIC_SAVE_AGENT_NAME;
  const sessionId = session?.id ?? null;
  const exportScope = useMemo(
    () =>
      saveAgentId && sessionId ? { agentId: saveAgentId, sessionId } : null,
    [saveAgentId, sessionId],
  );
  const interactions = useSessionInteractions({
    agentId: session?.agentId ?? null,
    sessionId,
    client: daemon.interactions,
    enabled:
      Boolean(daemon.interactions) &&
      session?.kind === 'chat' &&
      !composer.offline &&
      !missing,
    refreshKey: `${messages.length}:${runs.length}`,
    resetKey: session?.clearGeneration,
  });
  const sendWithSideQuestion = (override?: string) => {
    const text = override ?? composer.draft;
    if (
      !composer.literalDraft &&
      session?.kind === 'chat' &&
      /^\s*\/btw(?:\s|$)/.test(text)
    ) {
      const question = text.trimStart().slice(4).trim();
      if (!question) {
        composer.setDraft('/btw ');
        return;
      }
      void interactions.ask(question);
      composer.setDraft('');
      return;
    }
    composer.onSend(override);
  };
  const voiceKey =
    composer.conversationKey ??
    `${session?.agentId ?? agent.id}\u0000${sessionId ?? 'home'}`;
  const readAloud = useReadAloud(
    voiceKey,
    missing ||
      composer.disabled ||
      composer.offline ||
      composer.voiceDisabled === true,
  );
  const speaking = readAloud.speakingMessageId !== null;
  useEffect(() => {
    setPresenceSpeaking(speaking);
  }, [speaking]);
  useEffect(() => () => setPresenceSpeaking(false), []);
  const saveTarget = useMemo(
    () =>
      saveAgentId && sessionId
        ? { agentId: saveAgentId, agentName: saveAgentName, sessionId }
        : null,
    [saveAgentId, saveAgentName, sessionId],
  );
  const { save, savedState } = useSaveToMemory(saveTarget);
  // The usage line reads again when a run finishes.
  const finishedRuns = runs.filter((item) =>
    isTerminalRunStatus(item.run.status),
  ).length;
  const sessionDetails = useSessionDetails(
    saveTarget,
    `${agent.provider}:${agent.model}:${finishedRuns}:${messages.length}:${session?.summary?.createdAtMs ?? ''}:${session?.contextTrimmed?.atMs ?? ''}:${contextRefreshKey ?? ''}`,
    !composer.offline && !missing,
  );
  const transcriptActions = useMemo<TranscriptActions>(
    () => ({ ...actions, onSaveToMemory: save, savedToMemory: savedState }),
    [actions, save, savedState],
  );
  const conversation = useMemo(
    () => ({ ...agent, id: session?.agentId ?? agent.id, messages }),
    [agent, session?.agentId, messages],
  );
  const trimmedThrough =
    session?.contextTrimmed?.droppedThroughMessageId ?? null;
  // History is built apart from the runs and sends, so a streamed delta
  // rebuilds only what it changes and history items keep their identity.
  const history = useMemo(
    () => buildHistory({ messages, trimmedThrough, delegatedBy }),
    [messages, trimmedThrough, delegatedBy],
  );
  const items = useMemo(
    () => placeRuns(history, { runs, pending, olderHistory: hasOlder }),
    [history, runs, pending, hasOlder],
  );
  if (missing) {
    return (
      <section
        className="flex h-full min-h-0 flex-col items-center justify-center gap-3 p-6 text-center"
        aria-label="Session"
      >
        <p className="text-sm text-ink-2">This session was deleted.</p>
        <button type="button" className={ghostBtnCls} onClick={onNewChat}>
          Return to main chat
        </button>
      </section>
    );
  }
  // A run the transcript shows speaks for itself; the thinking indicator
  // covers active runs it does not know about (no stream).
  const thinking =
    composer.sending ||
    ((session?.activeRuns ?? 0) > 0 &&
      !runs.some((item) => !isTerminalRunStatus(item.run.status)));
  const footer = sessionFooter(session, telegramAvailable, discordAvailable);
  // Browser task and spoken replies are extra inputs: they live behind the
  // composer's + button, or stand alone when the session has no composer.
  const sessionTools = (
    <>
      {browserTask && (!session || session.kind === 'chat') && (
        <BrowserTaskControls
          key={voiceKey}
          agentId={agent.id}
          runId={
            runs
              .filter((item) =>
                runs.some(
                  ({ run }) =>
                    run.status === 'running' ||
                    run.status === 'awaiting_approval',
                )
                  ? item.run.status === 'running' ||
                    item.run.status === 'awaiting_approval'
                  : item.run.status !== 'queued',
              )
              .reduce<(typeof runs)[number] | null>(
                (latest, item) =>
                  !latest || item.run.createdAtMs > latest.run.createdAtMs
                    ? item
                    : latest,
                null,
              )?.run.id ?? null
          }
          disabled={
            composer.disabled || composer.sending || Boolean(composer.runActive)
          }
          offline={composer.offline}
          attachments={browserTask.attachments}
          selection={browserTask.selection}
          onSelection={browserTask.onSelection}
        />
      )}
      {session?.kind === 'chat' &&
        composer.nativeVoice?.agentId === session.agentId &&
        composer.nativeVoice.sessionId === session.id && (
          <NativeSpeech
            agentId={session.agentId}
            sessionId={session.id}
            conversationKey={voiceKey}
            messages={messages}
            disabled={
              missing ||
              composer.disabled ||
              composer.offline ||
              composer.sending ||
              Boolean(composer.runActive) ||
              composer.voiceDisabled === true
            }
          />
        )}
    </>
  );
  const renderMessages = (inlineResults?: ReactNode) => (<MessageList
        agent={conversation}
        items={items}
        actions={transcriptActions}
        sending={thinking}
        scrollerRef={scrollerRef}
        onSuggestion={onSuggestion}
        hasOlder={hasOlder}
        loadingOlder={loadingOlder}
        onLoadOlder={onLoadOlder}
        emptyState={
          session && session.kind !== 'chat' ? NO_MESSAGES : undefined
        }
        readAloud={readAloud}
        knownAttachments={knownAttachments}
        highlightMessageId={historyAnchor?.messageId}
        jumpRequestKey={jumpRequestKey}
        jumpReady={
          !historyAnchor || (!historyAnchor.loading && !historyAnchor.error)
        }
        onJumpToLatest={historyAnchor?.onReturnToLatest}
        onAtBottomChange={onAtBottomChange}
        tail={<>
            {inlineResults}
            {
          agent.workspaceRole !== 'helper' &&
          (!session ||
            (session.kind === 'chat' && session.agentId === agent.id)) ? (
            <ChatWorkflows
              key={`${agent.id}:${session?.id ?? 'new'}`}
              agent={agent}
              online={!composer.offline}
              refreshKey={`${messages.length}:${messages.at(-1)?.id ?? ''}:${runs.map(({ run }) => `${run.id}:${run.status}`).join(',')}:${session?.activeRuns ?? 0}:${composer.sending}:${contextRefreshKey ?? ''}`}
            />
          ) : undefined}
        </>}
      />);
  const view = (
    <section
      className="flex h-full min-h-0 flex-col"
      aria-label={session?.title ?? 'New chat'}
    >
      {/* The main chat's header is the top bar; rename, export and usage
       *  stay one / command away. */}
      {session && session.mainChat !== true ? (
        <SessionHeader
          session={session}
          onRename={onRename}
          onToggleArchived={onToggleArchived}
          onExport={onExport}
          automation={automation}
          onEditAutomation={onEditAutomation}
          usageLine={sessionUsageLine(sessionDetails.usage)}
        />
      ) : null}
      {session && (
        <SessionContinuity
          key={`${session.agentId}\u0000${session.id}`}
          session={session}
        />
      )}
      {notice}
      {session && extensions && <ChatExtensions key={`${session.agentId}:${session.id}:${session.clearGeneration ?? 0}`} session={session} request={extensions.request} onOpen={extensions.onOpen}
        onModelSaved={extensions.onModelSaved} refreshKey={contextRefreshKey} />}
      {session && controls && (
        <ConversationControls
          key={`${session.agentId}:${session.id}`}
          session={session}
          messages={messages}
          runs={runs.map((item) => item.run)}
          enabled={!composer.disabled && !composer.offline}
          {...controls}
        />
      )}
      {historyAnchor && (
        <div className="session-history-anchor">
          <p role={historyAnchor.error ? 'alert' : 'status'}>
            {historyAnchor.loading
              ? 'Loading saved message…'
              : (historyAnchor.error ??
                'Viewing a saved message in its conversation.')}
          </p>
          <button
            type="button"
            className={ghostBtnCls}
            onClick={historyAnchor.onReturnToLatest}
          >
            Return to latest messages
          </button>
        </div>
      )}
      {/* Mounted empty and filled when an error arrives, so it is
       *  announced (S3b-E). */}
      <div role="status">
        {session?.compactionError ? (
          <p className="px-4 pt-3 text-xs text-ink-3">
            Earlier messages could not be summarized:{' '}
            {session.compactionError.message}
          </p>
        ) : null}
      </div>
      {session?.kind === 'chat' && extensions && daemon.chatResults ? (
        <ChatResultsLayout agentId={session.agentId} sessionId={session.id} clearGeneration={session.clearGeneration ?? 0}
          refreshKey={`${contextRefreshKey}:${session.lastActivityAtMs}:${messages.length}:${runs.map(({run}) => `${run.id}:${run.status}`).join()}`}>
          {renderMessages}
        </ChatResultsLayout>
      ) : renderMessages()}
      {session?.kind === 'chat' && skillDrafts && (
        <SessionSkillDrafts
          sessionId={session.id}
          version={skillDrafts.version}
          epoch={skillDrafts.epoch}
          enabled={skillDrafts.enabled}
          onReview={skillDrafts.onReview}
        />
      )}
      {readAloud.error && (
        <p role="alert" className="px-4 text-sm text-danger">
          {readAloud.error}
        </p>
      )}
      {session?.kind === 'chat' && (
        <div className="space-y-3 px-4" aria-label="Conversation questions">
          {interactions.clarifications.map((request) => (
            <ClarificationCard
              key={request.id}
              request={request}
              onAnswer={interactions.answer}
            />
          ))}
          {interactions.sideQuestions.map((question) => (
            <SideQuestionCard
              key={question.id}
              question={question.question}
              pending={question.pending}
              answer={question.answer}
              error={question.error}
            />
          ))}
          {interactions.error && (
            <p role="status">
              Questions unavailable: {interactions.error}{' '}
              <button
                type="button"
                className={ghostBtnCls}
                onClick={interactions.refresh}
              >
                Retry questions
              </button>
            </p>
          )}
        </div>
      )}
      {footer.kind !== 'composer' && (
        <div className="session-tools">{sessionTools}</div>
      )}
      {footer.kind === 'composer' && session && (
        <ContextNote
          agentName={agent.name}
          usage={
            composer.offline
              ? undefined
              : sessionDetails.settled
                ? sessionDetails.contextUsage
                : session.contextUsage
          }
        />
      )}
      {footer.kind === 'composer' ? (
        <Composer
          agentName={agent.name}
          label={footer.label}
          draft={composer.draft}
          setDraft={composer.setDraft}
          sending={composer.sending}
          disabled={composer.disabled}
          offline={composer.offline}
          onSend={sendWithSideQuestion}
          error={composer.error}
          onDismissError={composer.onDismissError}
          recovery={composer.recovery}
          commands={
            composer.literalDraft
              ? undefined
              : session?.kind === 'discord'
                ? composer.commands?.filter((command) => !command.skill)
                : session?.kind === 'chat'
                  ? [
                      ...(composer.commands ?? []),
                      {
                        name: 'btw',
                        description: 'Ask a side question while work continues',
                        needs: 'a question',
                        placeholder: '<question>',
                      },
                    ]
                  : composer.commands
          }
          runActive={composer.runActive}
          onStop={composer.onStop}
          onSteer={composer.onSteer}
          conversationKey={voiceKey}
          voiceDisabled={composer.voiceDisabled}
          nativeVoice={
            !session || session.kind === 'chat'
              ? composer.nativeVoice
              : undefined
          }
          attachments={
            session?.kind === 'discord' ? undefined : composer.attachments
          }
          savedAttachmentCount={composer.savedAttachmentCount}
          mode={
            session?.kind === 'chat' || !session ? composer.mode : undefined
          }
          model={composer.model}
          tools={
            <>
              {sessionTools}
              {session?.kind === 'chat' &&
                /^\s*\/btw\s+\S/.test(composer.draft) && (
                  <button
                    type="button"
                    className={ghostBtnCls}
                    disabled={composer.offline || composer.disabled}
                    onClick={() => sendWithSideQuestion()}
                  >
                    Ask side question
                  </button>
                )}
            </>
          }
        />
      ) : (
        <div className="session-footer-note" role="note">
          <p>{footer.text}</p>
          {session?.kind === 'discord' &&
            session.capabilities.stop &&
            composer.onStop && (
              <button
                type="button"
                className={ghostBtnCls}
                onClick={composer.onStop}
              >
                Stop
              </button>
            )}
          {footer.action === 'work' && (
            <button type="button" className={ghostBtnCls} onClick={onOpenWork}>
              Open Work
            </button>
          )}
        </div>
      )}
      <p className="sr-only" aria-live="polite" aria-atomic="true">
        {announcement}
      </p>
    </section>
  );
  return (
    <ChatExportScope.Provider value={exportScope}>
      {view}
    </ChatExportScope.Provider>
  );
}

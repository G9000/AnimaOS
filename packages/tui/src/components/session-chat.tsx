import React, { useLayoutEffect, useRef, useState } from 'react';
import { Box, Text, useInput } from 'ink';
import {
  isTerminalRunStatus,
  type Run,
  type RunMode,
  type Session,
  type SessionMessage,
} from '@animaos/sdk';
import {
  mergeSessionMessages,
  type SessionEventState,
} from '../session-state.js';

const STOPPABLE_STATUSES = new Set<Run['status']>([
  'queued',
  'running',
  'awaiting_approval',
]);

export interface SessionChatProps {
  session: Session;
  messages: SessionMessage[];
  runs: Run[];
  activeRun: Run | null;
  eventState: SessionEventState | null;
  runMode: RunMode;
  canStopActiveRun: boolean;
  stopRequested: boolean;
  disabled?: boolean;
  loadingMessages?: boolean;
  streamStatus?: 'disconnected' | 'connecting' | 'connected' | 'reconnecting';
  error?: string | null;
  visible?: boolean;
  reviewSummary?: { approvals: number; clarifications: number };
  onReview?: () => void;
  onSubmit: (text: string) => void | Promise<unknown>;
  onModeChange: (mode: RunMode) => void;
  onStop: () => void | Promise<unknown>;
}

function chronologicalMessages(messages: SessionMessage[]): SessionMessage[] {
  return mergeSessionMessages([], messages).filter(
    (message) => !message.hidden,
  );
}

function messageRole(role: SessionMessage['role']): string {
  return role === 'tool' ? 'tool' : role;
}

export function SessionChat({
  session,
  messages,
  runs,
  activeRun,
  eventState,
  runMode,
  canStopActiveRun,
  stopRequested,
  disabled = false,
  loadingMessages = false,
  streamStatus,
  error = null,
  visible = true,
  reviewSummary,
  onReview,
  onSubmit,
  onModeChange,
  onStop,
}: SessionChatProps): React.ReactElement | null {
  const [draft, setDraft] = useState('');
  const sessionIdRef = useRef(session.id);
  useLayoutEffect(() => {
    sessionIdRef.current = session.id;
  }, [session.id]);
  const [draftSessionId, setDraftSessionId] = useState(session.id);
  if (draftSessionId !== session.id) {
    setDraftSessionId(session.id);
    setDraft('');
  }
  const currentRun =
    activeRun ??
    [...runs].sort((left, right) => right.createdAtMs - left.createdAtMs)[0] ??
    null;
  const canSwitchMode =
    !disabled && (runMode !== 'queue' || session.capabilities.steer);
  const canStop = Boolean(
    !disabled &&
    !stopRequested &&
    canStopActiveRun &&
    currentRun &&
    STOPPABLE_STATUSES.has(currentRun.status) &&
    currentRun.stop === null,
  );
  const showStopRequested = currentRun
    ? !isTerminalRunStatus(currentRun.status) &&
      (stopRequested || currentRun.stop !== null)
    : stopRequested;
  const sendDisabled =
    disabled ||
    !session.capabilities.send ||
    (runMode === 'steer' && !session.capabilities.steer);
  const displayedMessages = chronologicalMessages(messages);
  const hasLiveRun = Boolean(
    currentRun && !isTerminalRunStatus(currentRun.status),
  );
  const liveSteps =
    hasLiveRun && currentRun
      ? Object.entries(eventState?.steps ?? {}).filter(
          ([, step]) => step.runId === currentRun.id && step.text.length > 0,
        )
      : [];
  const liveTools =
    hasLiveRun && currentRun
      ? Object.values(eventState?.tools ?? {}).filter(
          (tool) => tool.runId === currentRun.id,
        )
      : [];

  useInput(
    (input, key) => {
      if (key.ctrl && input.toLowerCase() === 'r') {
        onReview?.();
        return;
      }
      if (key.ctrl && input.toLowerCase() === 'x') {
        if (canStop) onStop();
        return;
      }
      if (key.tab) {
        if (canSwitchMode)
          onModeChange(runMode === 'queue' ? 'steer' : 'queue');
        return;
      }
      if (sendDisabled) return;
      if (key.return) {
        const text = draft.trim();
        if (!text) return;
        const submittedSessionId = session.id;
        const result = onSubmit(text);
        setDraft('');
        void Promise.resolve(result).then(
          (accepted) => {
            if (
              accepted === false &&
              sessionIdRef.current === submittedSessionId
            ) {
              setDraft((current) => (current.length === 0 ? text : current));
            }
          },
          () => {
            if (sessionIdRef.current === submittedSessionId) {
              setDraft((current) => (current.length === 0 ? text : current));
            }
          },
        );
        return;
      }
      if (key.backspace || key.delete) {
        setDraft((current) => current.slice(0, -1));
        return;
      }
      if (
        !key.ctrl &&
        !key.meta &&
        !key.escape &&
        !key.tab &&
        input.length > 0
      ) {
        setDraft((current) => current + input);
      }
    },
    { isActive: visible },
  );

  if (!visible) return null;

  return (
    <Box flexDirection="column" paddingX={1}>
      <Box borderStyle="single" paddingX={1}>
        <Text bold color="cyan">
          {session.title || 'Untitled session'}
        </Text>
        <Text color="gray">
          {'  '}Agent: {session.agentId}
        </Text>
      </Box>

      {error ? (
        <Box marginTop={1}>
          <Text color="red">{error}</Text>
        </Box>
      ) : null}

      <Box marginTop={1}>
        <Text bold color={currentRun ? 'yellow' : 'gray'}>
          Run: {currentRun?.status ?? 'idle'}
        </Text>
        {streamStatus ? (
          <Text color="gray">
            {'  '}Stream: {streamStatus}
          </Text>
        ) : null}
        {showStopRequested ? (
          <Text color="yellow">{'  '}Stop requested</Text>
        ) : null}
        {canStop ? <Text color="yellow">{'  '}ctrl+x stop</Text> : null}
      </Box>

      <Box flexDirection="column" marginTop={1}>
        {reviewSummary ? (
          <Text
            color={
              reviewSummary.approvals + reviewSummary.clarifications > 0
                ? 'yellow'
                : 'gray'
            }
          >
            Review: {reviewSummary.approvals} approval
            {reviewSummary.approvals === 1 ? '' : 's'} ·{' '}
            {reviewSummary.clarifications} clarification
            {reviewSummary.clarifications === 1 ? '' : 's'} · ctrl+r or /review
          </Text>
        ) : null}
        {loadingMessages && displayedMessages.length === 0 ? (
          <Text color="gray">Loading transcript…</Text>
        ) : null}
        {!loadingMessages && displayedMessages.length === 0 ? (
          <Text color="gray">No messages yet.</Text>
        ) : null}
        {displayedMessages.map((message) => (
          <Box key={message.id} flexDirection="column" marginBottom={1}>
            <Text color={message.role === 'assistant' ? 'cyan' : 'white'} bold>
              {messageRole(message.role)}:
            </Text>
            {message.text ? <Text wrap="wrap">{message.text}</Text> : null}
            {message.attachments.length > 0 ? (
              <Text color="gray">
                {message.attachments
                  .map((attachment) => attachment.name)
                  .join(', ')}
              </Text>
            ) : null}
          </Box>
        ))}
      </Box>

      {currentRun && liveSteps.length > 0 ? (
        <Box flexDirection="column" marginTop={1}>
          <Text bold color="cyan">
            Live assistant output
          </Text>
          {liveSteps.map(([stepId, step]) => (
            <Text key={stepId} wrap="wrap">
              {step.text}
            </Text>
          ))}
        </Box>
      ) : null}

      {hasLiveRun && currentRun && eventState?.progressByRun[currentRun.id] ? (
        <Box marginTop={1}>
          <Text color="yellow">
            Progress: {eventState.progressByRun[currentRun.id]}
          </Text>
        </Box>
      ) : null}

      {liveTools.length > 0 ? (
        <Box flexDirection="column" marginTop={1}>
          <Text bold color="magenta">
            Tool activity
          </Text>
          {liveTools.map((tool) => (
            <Box key={`${tool.runId}:${tool.stepId}:${tool.toolCallId}`}>
              <Text color={tool.status === 'running' ? 'yellow' : 'gray'}>
                {tool.name} · {tool.status}
              </Text>
              {tool.resultPreview ? (
                <Text color="gray">
                  {'  '}
                  {tool.resultPreview}
                </Text>
              ) : null}
            </Box>
          ))}
        </Box>
      ) : null}

      <Box marginTop={1} borderStyle="single" paddingX={1}>
        <Box flexDirection="column" flexGrow={1}>
          <Text color={sendDisabled ? 'gray' : 'white'}>
            {draft ||
              (sendDisabled
                ? runMode === 'steer' && !session.capabilities.steer
                  ? 'Steer mode is unavailable for this session.'
                  : 'Sending is unavailable.'
                : 'Type a message…')}
            {!sendDisabled ? <Text color="cyan">▌</Text> : null}
          </Text>
          <Text color="gray" dimColor>
            Mode: {runMode}
            {canSwitchMode ? ' · tab switch mode' : ''}
            {sendDisabled ? '' : ' · enter send'}
            {canStop ? ' · ctrl+x stop' : ''}
          </Text>
        </Box>
      </Box>
    </Box>
  );
}

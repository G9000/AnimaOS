import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Box, Text, useInput } from 'ink';
import type { AgentSummary, Session } from '@animaos/sdk';

type PickerPhase = 'agents' | 'sessions';

export interface SessionListProps {
  agents: AgentSummary[];
  selectedAgentId: string | null;
  sessions: Session[];
  nextCursor: string | null;
  loadingAgents?: boolean;
  loading?: boolean;
  error?: string | null;
  disabled?: boolean;
  onSelectAgent: (agentId: string) => void | Promise<unknown>;
  onOpenSession: (session: Session) => void | Promise<unknown>;
  onCreateSession: () => void | Promise<unknown>;
  onLoadNext: (cursor: string) => void | Promise<unknown>;
  onRetry?: () => void | Promise<unknown>;
  onBack: () => void;
}

function formatActivity(timestamp: number): string {
  if (!Number.isFinite(timestamp)) return 'activity time unavailable';
  const value = new Date(timestamp);
  if (Number.isNaN(value.getTime())) return 'activity time unavailable';
  return `${value.toISOString().slice(0, 16).replace('T', ' ')} UTC`;
}

function countLabel(count: number, singular: string, plural = `${singular}s`) {
  return `${String(count)} ${count === 1 ? singular : plural}`;
}

export function SessionList({
  agents,
  selectedAgentId,
  sessions,
  nextCursor,
  loadingAgents = false,
  loading = false,
  error = null,
  disabled = false,
  onSelectAgent,
  onOpenSession,
  onCreateSession,
  onLoadNext,
  onRetry,
  onBack,
}: SessionListProps): React.ReactElement {
  const [phase, setPhase] = useState<PickerPhase>(
    selectedAgentId === null && agents.length > 1 ? 'agents' : 'sessions',
  );
  const visibleSessions = useMemo(
    () =>
      sessions.filter(
        (session) => session.kind === 'chat' && !session.archived,
      ),
    [sessions],
  );
  const [agentChoiceId, setAgentChoiceId] = useState(
    () =>
      agents.find((agent) => agent.state.id === selectedAgentId)?.state.id ??
      agents[0]?.state.id ??
      null,
  );
  const [selectedSessionId, setSelectedSessionId] = useState(
    () => visibleSessions[0]?.id ?? null,
  );
  const previousSelectedAgentIdRef = useRef(selectedAgentId);
  const selectedAgent = agents.find(
    (agent) => agent.state.id === selectedAgentId,
  );

  useEffect(() => {
    const selectionChanged =
      previousSelectedAgentIdRef.current !== selectedAgentId;
    previousSelectedAgentIdRef.current = selectedAgentId;
    setAgentChoiceId((current) => {
      if (selectionChanged && selectedAgentId) return selectedAgentId;
      if (current && agents.some((agent) => agent.state.id === current)) {
        return current;
      }
      if (
        selectedAgentId &&
        agents.some((agent) => agent.state.id === selectedAgentId)
      ) {
        return selectedAgentId;
      }
      return agents[0]?.state.id ?? null;
    });
    setSelectedSessionId((current) =>
      current && visibleSessions.some((session) => session.id === current)
        ? current
        : (visibleSessions[0]?.id ?? null),
    );
    if (selectedAgentId === null && agents.length > 1) {
      setPhase('agents');
    }
  }, [agents, phase, selectedAgentId, visibleSessions]);

  const choosingAgent =
    (phase === 'agents' || selectedAgentId === null) && agents.length > 1;
  const selectedAgentIndex = agents.findIndex(
    (agent) => agent.state.id === agentChoiceId,
  );
  const footer = choosingAgent
    ? `↑↓ choose agent · ${disabled ? 'actions unavailable' : 'enter select'} · q back`
    : [
        '↑↓ choose',
        !disabled ? 'enter open' : null,
        !disabled && selectedAgentId ? 'c create' : null,
        nextCursor && !loading && !disabled ? 'n more' : null,
        agents.length > 1 ? 'a agent' : null,
        error && onRetry && !disabled ? 'r retry' : null,
        'q back',
      ]
        .filter((hint): hint is string => Boolean(hint))
        .join(' · ');

  useInput((input, key) => {
    if (key.escape || input.toLowerCase() === 'q') {
      onBack();
      return;
    }
    if (input.toLowerCase() === 'r' && !disabled && error && onRetry) {
      onRetry();
      return;
    }

    if (choosingAgent) {
      if (key.upArrow) {
        setAgentChoiceId((current) => {
          const currentIndex = agents.findIndex(
            (agent) => agent.state.id === current,
          );
          return agents[Math.max(0, currentIndex - 1)]?.state.id ?? null;
        });
      } else if (key.downArrow) {
        setAgentChoiceId((current) => {
          const currentIndex = agents.findIndex(
            (agent) => agent.state.id === current,
          );
          return (
            agents[Math.min(agents.length - 1, currentIndex + 1)]?.state.id ??
            null
          );
        });
      } else if (key.return && !disabled) {
        const agent = agents.find(
          (candidate) => candidate.state.id === agentChoiceId,
        );
        if (agent) {
          setPhase('sessions');
          onSelectAgent(agent.state.id);
        }
      }
      return;
    }

    if (input.toLowerCase() === 'a' && agents.length > 1) {
      setPhase('agents');
      return;
    }
    if (key.upArrow) {
      setSelectedSessionId((current) => {
        const currentIndex = visibleSessions.findIndex(
          (session) => session.id === current,
        );
        return visibleSessions[Math.max(0, currentIndex - 1)]?.id ?? null;
      });
      return;
    }
    if (key.downArrow) {
      setSelectedSessionId((current) => {
        const currentIndex = visibleSessions.findIndex(
          (session) => session.id === current,
        );
        return (
          visibleSessions[
            Math.min(Math.max(visibleSessions.length - 1, 0), currentIndex + 1)
          ]?.id ?? null
        );
      });
      return;
    }
    if (key.return && !disabled) {
      const session = visibleSessions.find(
        (candidate) => candidate.id === selectedSessionId,
      );
      if (session) onOpenSession(session);
      return;
    }
    if (input.toLowerCase() === 'c' && !disabled && selectedAgentId) {
      onCreateSession();
      return;
    }
    if (input.toLowerCase() === 'n' && !disabled && !loading && nextCursor) {
      onLoadNext(nextCursor);
      return;
    }
  });

  return (
    <Box flexDirection="column" paddingX={1}>
      <Box borderStyle="single" paddingX={1}>
        <Text bold color="cyan">
          {choosingAgent ? 'Choose an agent' : 'Chat sessions'}
        </Text>
        {selectedAgent && !choosingAgent ? (
          <Text color="gray">
            {'  '}Agent: {selectedAgent.state.name}
          </Text>
        ) : null}
      </Box>

      {error ? (
        <Box marginTop={1}>
          <Text color="red">{error}</Text>
        </Box>
      ) : null}

      {choosingAgent ? (
        <Box flexDirection="column" marginTop={1}>
          {loadingAgents ? <Text color="gray">Loading agents…</Text> : null}
          {agents.map((agent, index) => {
            const active = index === selectedAgentIndex;
            return (
              <Box key={agent.state.id}>
                <Text color={active ? 'magenta' : 'gray'} bold={active}>
                  {active ? '❯ ' : '  '}
                  {agent.state.name}
                </Text>
                <Text color="gray">
                  {'  '}
                  {agent.state.id}
                </Text>
              </Box>
            );
          })}
          {agents.length === 0 ? (
            <Text color="yellow">No daemon agents are available.</Text>
          ) : null}
        </Box>
      ) : (
        <Box flexDirection="column" marginTop={1}>
          {loadingAgents ? <Text color="gray">Loading agents…</Text> : null}
          {agents.length === 0 ? (
            !loadingAgents && !error ? (
              <Text color="yellow">
                No daemon agents are available. Configure an agent before
                starting chat.
              </Text>
            ) : null
          ) : null}
          {loading && visibleSessions.length === 0 ? (
            <Text color="gray">Loading sessions…</Text>
          ) : null}
          {!loading &&
          !error &&
          agents.length > 0 &&
          visibleSessions.length === 0 ? (
            <Text color="gray">
              No chat sessions yet. Press c to create one.
            </Text>
          ) : null}
          {visibleSessions.map((session, index) => {
            const active = session.id === selectedSessionId;
            return (
              <Box key={session.id} flexDirection="column" marginBottom={1}>
                <Box>
                  <Text color={active ? 'magenta' : 'gray'} bold={active}>
                    {active ? '❯ ' : '  '}
                    {session.title || 'Untitled session'}
                  </Text>
                </Box>
                <Box marginLeft={2}>
                  <Text color="gray">
                    Updated {formatActivity(session.lastActivityAtMs)}
                  </Text>
                  {session.activeRuns > 0 ? (
                    <Text color="yellow">
                      {'  ·  '}
                      {countLabel(session.activeRuns, 'active run')}
                    </Text>
                  ) : null}
                  {session.pendingApprovals > 0 ? (
                    <Text color="yellow">
                      {'  ·  '}
                      {countLabel(session.pendingApprovals, 'pending approval')}
                    </Text>
                  ) : null}
                </Box>
                {session.preview ? (
                  <Box marginLeft={2}>
                    <Text color="gray" dimColor>
                      {session.preview}
                    </Text>
                  </Box>
                ) : null}
              </Box>
            );
          })}
          {loading && visibleSessions.length > 0 ? (
            <Text color="gray">Loading more sessions…</Text>
          ) : null}
          {disabled ? (
            <Text color="yellow">
              Session actions are unavailable while offline.
            </Text>
          ) : null}
        </Box>
      )}

      <Box marginTop={1}>
        <Text color="gray" dimColor>
          {footer}
        </Text>
      </Box>
    </Box>
  );
}

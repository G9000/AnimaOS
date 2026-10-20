import type {
  ClarificationAnswerInput,
  ClarificationRequest,
  InteractionsClient,
  SideQuestionAnswer,
} from '@animaOS-SWARM/sdk';
import { useCallback, useEffect, useRef, useState } from 'react';

export interface SessionInteractionOptions {
  agentId: string | null;
  sessionId: string | null;
  client: Pick<InteractionsClient, 'list' | 'answer' | 'sideQuestion'>;
  enabled?: boolean;
  refreshKey?: string | number;
}
export interface SideQuestionView {
  id: number;
  question: string;
  pending: boolean;
  answer: SideQuestionAnswer | null;
  error: string | null;
}
export function useSessionInteractions(options: SessionInteractionOptions) {
  const { agentId, sessionId, client, enabled = true, refreshKey } = options;
  const key =
    enabled && agentId && sessionId ? `${agentId}\u0000${sessionId}` : null;
  const latest = useRef({ key, agentId, sessionId, client });
  latest.current = { key, agentId, sessionId, client };
  const [read, setRead] = useState<{
    key: string | null;
    requests: ClarificationRequest[];
    error: string | null;
  }>({ key: null, requests: [], error: null });
  const [sides, setSides] = useState<{
    key: string | null;
    questions: SideQuestionView[];
  }>({ key: null, questions: [] });
  const [revision, setRevision] = useState(0);
  const nextSide = useRef(0);
  const sideCalls = useRef(new Map<number, AbortController>());
  const refresh = useCallback(() => setRevision((value) => value + 1), []);

  useEffect(() => {
    if (!key || !agentId || !sessionId) return;
    let active = true;
    let inFlight = false;
    const abort = new AbortController();
    const readRequests = async () => {
      if (inFlight) return;
      inFlight = true;
      try {
        const requests = await client.list(agentId, sessionId, {
          signal: abort.signal,
        });
        if (active && latest.current.key === key)
          setRead({ key, requests, error: null });
      } catch (caught) {
        if (active && latest.current.key === key)
          setRead((current) => ({
            key,
            requests: current.key === key ? current.requests : [],
            error:
              caught instanceof Error
                ? caught.message
                : 'Questions could not be loaded.',
          }));
      } finally {
        inFlight = false;
      }
    };
    void readRequests();
    // Questions may arrive while the run is still inside a tool call. A
    // bounded read also recovers them when the stream is disconnected.
    const interval = window.setInterval(() => void readRequests(), 2000);
    return () => {
      active = false;
      abort.abort();
      window.clearInterval(interval);
    };
  }, [key, agentId, sessionId, client, revision, refreshKey]);

  useEffect(() => {
    const calls = sideCalls.current;
    return () => {
      for (const call of calls.values()) call.abort();
      calls.clear();
    };
  }, [key]);

  const ask = useCallback(async (question: string) => {
    const scope = latest.current;
    if (!scope.key || !scope.agentId || !scope.sessionId || !question.trim())
      return;
    const id = ++nextSide.current;
    const abort = new AbortController();
    sideCalls.current.set(id, abort);
    setSides((current) => ({
      key: scope.key,
      questions: [
        ...(current.key === scope.key ? current.questions : []),
        {
          id,
          question: question.trim(),
          pending: true,
          answer: null,
          error: null,
        },
      ].slice(-5),
    }));
    const settle = (patch: Partial<SideQuestionView>) => {
      if (latest.current.key !== scope.key || abort.signal.aborted) return;
      setSides((current) =>
        current.key !== scope.key
          ? current
          : {
              ...current,
              questions: current.questions.map((item) =>
                item.id === id ? { ...item, pending: false, ...patch } : item,
              ),
            },
      );
    };
    try {
      settle({
        answer: await scope.client.sideQuestion(
          scope.agentId,
          scope.sessionId,
          question.trim(),
          { signal: abort.signal },
        ),
      });
    } catch (caught) {
      settle({
        error:
          caught instanceof Error
            ? caught.message
            : 'The side question could not be answered.',
      });
    } finally {
      sideCalls.current.delete(id);
    }
  }, []);

  const answer = useCallback(
    async (request: ClarificationRequest, input: ClarificationAnswerInput) => {
      const scope = latest.current;
      if (
        !scope.key ||
        request.agentId !== scope.agentId ||
        request.sessionId !== scope.sessionId
      )
        throw new Error('This question belongs to another session.');
      const result = await scope.client.answer(
        request.agentId,
        request.sessionId,
        request.id,
        input,
      );
      if (latest.current.key === scope.key) {
        setRead((current) =>
          current.key === scope.key
            ? {
                ...current,
                requests: current.requests.map((item) =>
                  item.id === result.id ? result : item,
                ),
              }
            : current,
        );
        refresh();
      }
      return result;
    },
    [refresh],
  );

  return {
    clarifications: read.key === key ? read.requests : [],
    sideQuestions: sides.key === key ? sides.questions : [],
    error: read.key === key ? read.error : null,
    ask,
    answer,
    refresh,
  };
}

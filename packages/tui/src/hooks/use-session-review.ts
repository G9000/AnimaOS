import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  AgentEvent,
  Approval,
  ApprovalDecisionInput,
  ClarificationRequest,
  ClarificationAnswerInput,
} from '@animaos/sdk';
import type { TuiReviewApi } from '../session-api.js';
import {
  approvalInputError,
  mergeReviewRecords,
} from '../session-review-state.js';

export interface SessionReviewOptions {
  api?: TuiReviewApi;
  agentId?: string | null;
  sessionId?: string | null;
  online?: boolean;
  runAgentIds?: Record<string, string>;
  runSessionIds?: Record<string, string>;
}

export interface SessionReview {
  available: boolean;
  approvals: Approval[];
  clarifications: ClarificationRequest[];
  canAct: boolean;
  loading: boolean;
  error: string | null;
  notice: string | null;
  busyIds: string[];
  formVersions: Record<string, number>;
  approvalBlocked(item: Approval): boolean;
  clarificationBlocked(item: ClarificationRequest): boolean;
  refresh(): Promise<void>;
  onEvent(event: AgentEvent): void;
  decide(item: Approval, input: ApprovalDecisionInput): Promise<boolean>;
  answer(
    item: ClarificationRequest,
    input: ClarificationAnswerInput,
  ): Promise<boolean>;
}

interface ReviewStore {
  key: string;
  api?: TuiReviewApi;
  approvals: Approval[];
  clarifications: ClarificationRequest[];
  approvalVersion: number;
  clarificationVersion: number;
  approvalsReady: boolean;
  clarificationsReady: boolean;
  approvalError: string | null;
  clarificationError: string | null;
  actionError: string | null;
  notice: string | null;
  busy: Set<string>;
  blocked: Set<string>;
  helpers: Record<string, string>;
  formVersions: Record<string, number>;
}

const message = (error: unknown) =>
  error instanceof Error ? error.message : String(error);
const requestKey = (kind: string, item: { id: string; revision: number }) =>
  `${kind}:${item.id}:${item.revision}`;

export function useSessionReview({
  api,
  agentId,
  sessionId,
  online = true,
  runAgentIds = {},
  runSessionIds = {},
}: SessionReviewOptions): SessionReview {
  const [, render] = useState(0);
  const key = JSON.stringify([agentId, sessionId]);
  const storeRef = useRef<ReviewStore | null>(null);
  if (
    !storeRef.current ||
    storeRef.current.key !== key ||
    storeRef.current.api !== api
  ) {
    storeRef.current = {
      key,
      api,
      approvals: [],
      clarifications: [],
      approvalVersion: 0,
      clarificationVersion: 0,
      approvalsReady: false,
      clarificationsReady: false,
      approvalError: null,
      clarificationError: null,
      actionError: null,
      notice: null,
      busy: new Set(),
      blocked: new Set(),
      helpers: {},
      formVersions: {},
    };
  }
  const store = storeRef.current;
  const onlineRef = useRef(online);
  onlineRef.current = online;
  const ownersRef = useRef({ runAgentIds, runSessionIds, agentId, sessionId });
  ownersRef.current = { runAgentIds, runSessionIds, agentId, sessionId };
  const refreshRef = useRef<() => Promise<void>>(async () => {});
  const notify = useCallback((target: ReviewStore) => {
    if (storeRef.current === target) render((value) => value + 1);
  }, []);

  const belongs = useCallback(
    (
      item: { agentId: string; sessionId: string; runId: string },
      target: ReviewStore,
    ) => {
      const scope = ownersRef.current;
      return (
        item.sessionId === scope.sessionId &&
        (item.agentId === scope.agentId ||
          (scope.runSessionIds[item.runId] === scope.sessionId &&
            scope.runAgentIds[item.runId] === item.agentId) ||
          target.helpers[item.runId] === item.agentId)
      );
    },
    [],
  );

  const helperOwners = [
    ...new Set(
      Object.entries(runAgentIds)
        .filter(([id]) => runSessionIds[id] === sessionId)
        .map(([, owner]) => owner),
    ),
  ]
    .sort()
    .join('\u0000');

  useEffect(() => {
    refreshRef.current = async () => {};
    store.approvalsReady = false;
    store.clarificationsReady = false;
    notify(store);
    if (!api || !agentId || !sessionId || !online) return;
    const controller = new AbortController();
    const valid = () =>
      storeRef.current === store && !controller.signal.aborted;
    let inFlight: Promise<void> | null = null;
    const load = (): Promise<void> => {
      if (inFlight) return inFlight;
      const versionA = store.approvalVersion;
      const versionC = store.clarificationVersion;
      const helperAgentIds = Object.entries(ownersRef.current.runAgentIds)
        .filter(([id]) => ownersRef.current.runSessionIds[id] === sessionId)
        .map(([, id]) => id);
      const agents = [
        ...new Set([
          agentId,
          ...helperAgentIds,
          ...Object.values(store.helpers),
        ]),
      ];
      inFlight = Promise.all([
        api.listPendingApprovals(controller.signal).then(
          (items) => {
            if (!valid()) return;
            if (store.approvalVersion === versionA) {
              const scoped = items.filter((item) => belongs(item, store));
              const ids = new Set(scoped.map((item) => item.id));
              store.approvals = mergeReviewRecords(
                store.approvals,
                scoped,
              ).filter((item) => item.status !== 'pending' || ids.has(item.id));
              for (const item of scoped)
                store.blocked.delete(requestKey('a', item));
            }
            store.approvalsReady = true;
            store.approvalError = null;
          },
          (error) => {
            if (valid()) store.approvalError = `Approvals: ${message(error)}`;
          },
        ),
        Promise.all(
          agents.map((owner) =>
            api.listClarifications(owner, sessionId, controller.signal),
          ),
        ).then(
          (pages) => {
            if (!valid()) return;
            if (store.clarificationVersion === versionC) {
              const scoped = pages
                .flat()
                .filter((item) => belongs(item, store));
              const ids = new Set(scoped.map((item) => item.id));
              store.clarifications = mergeReviewRecords(
                store.clarifications,
                scoped,
              ).filter((item) => item.status !== 'pending' || ids.has(item.id));
              for (const item of scoped)
                store.blocked.delete(requestKey('c', item));
            }
            store.clarificationsReady = true;
            store.clarificationError = null;
          },
          (error) => {
            if (valid())
              store.clarificationError = `Clarifications: ${message(error)}`;
          },
        ),
      ])
        .then(() => {
          if (valid()) notify(store);
        })
        .finally(() => {
          inFlight = null;
        });
      return inFlight;
    };
    refreshRef.current = async () => {
      if (inFlight) await inFlight;
      if (valid()) await load();
    };
    void load();
    const timer = setInterval(() => {
      void load();
    }, 2_000);
    return () => {
      controller.abort();
      clearInterval(timer);
    };
  }, [api, agentId, sessionId, online, store, helperOwners, belongs, notify]);

  const onEvent = useCallback(
    (event: AgentEvent) => {
      const current = storeRef.current!;
      const scope = ownersRef.current;
      if (!scope.agentId || !scope.sessionId) return;
      if (event.type === 'stream.snapshot' && event.agentId === scope.agentId) {
        for (const { run } of event.runs) {
          if (run.sessionId === scope.sessionId)
            current.helpers[run.id] = run.agentId;
        }
        current.approvalVersion++;
        current.approvals = mergeReviewRecords(
          current.approvals,
          event.approvals.filter((item) => belongs(item, current)),
        );
        notify(current);
        void refreshRef.current();
      } else if (
        event.type === 'approval.requested' ||
        event.type === 'approval.resolved'
      ) {
        if (!belongs(event.approval, current)) return;
        current.approvalVersion++;
        current.approvals = mergeReviewRecords(current.approvals, [
          event.approval,
        ]);
        notify(current);
      } else if (
        event.type === 'stream.resync' &&
        event.agentId === scope.agentId
      ) {
        void refreshRef.current();
      }
    },
    [belongs, notify],
  );

  const approvalBlocked = (item: Approval) =>
    !api ||
    !online ||
    !store.approvalsReady ||
    Boolean(store.approvalError) ||
    item.status !== 'pending' ||
    item.expiresAtMs <= Date.now() ||
    store.blocked.has(requestKey('a', item)) ||
    store.busy.has(`a:${item.id}`);
  const clarificationBlocked = (item: ClarificationRequest) =>
    !api ||
    !online ||
    !store.clarificationsReady ||
    Boolean(store.clarificationError) ||
    item.status !== 'pending' ||
    store.blocked.has(requestKey('c', item)) ||
    store.busy.has(`c:${item.id}`);

  const write = async (
    kind: 'a' | 'c',
    item: Approval | ClarificationRequest,
    input: ApprovalDecisionInput | ClarificationAnswerInput,
  ): Promise<boolean> => {
    const target = storeRef.current!;
    const current = (
      kind === 'a' ? target.approvals : target.clarifications
    ).find((record) => record.id === item.id);
    const busyKey = `${kind}:${item.id}`;
    const blocked =
      kind === 'a'
        ? approvalBlocked(item as Approval)
        : clarificationBlocked(item as ClarificationRequest);
    if (
      !api ||
      !onlineRef.current ||
      target !== store ||
      blocked ||
      target.busy.has(busyKey) ||
      !belongs(item, target) ||
      !current ||
      current.revision !== item.revision ||
      current.status !== 'pending' ||
      input.revision !== item.revision
    )
      return false;
    const invalid =
      kind === 'a'
        ? approvalInputError(item as Approval, input as ApprovalDecisionInput)
        : null;
    if (invalid) {
      target.actionError = invalid;
      notify(target);
      return false;
    }
    target.busy.add(busyKey);
    target.actionError = null;
    target.notice = null;
    notify(target);
    try {
      if (kind === 'a') {
        const updated = await api.decideApproval(
          item.id,
          input as ApprovalDecisionInput,
        );
        if (storeRef.current === target) {
          target.approvalVersion++;
          target.approvals = mergeReviewRecords(target.approvals, [updated]);
          target.notice = 'Approval decision saved.';
        }
      } else {
        const updated = await api.answerClarification(
          item.agentId,
          item.sessionId,
          item.id,
          input as ClarificationAnswerInput,
        );
        if (storeRef.current === target) {
          target.clarificationVersion++;
          target.clarifications = mergeReviewRecords(target.clarifications, [
            updated,
          ]);
          target.notice =
            'Answers saved. Interrupted work may still require recovery review.';
        }
      }
      return true;
    } catch (error) {
      if (storeRef.current === target) {
        const status =
          (error as { status?: number; statusCode?: number } | null)?.status ??
          (error as { statusCode?: number } | null)?.statusCode;
        if (status === 404 || status === 409) {
          target.blocked.add(requestKey(kind, item));
          target.formVersions[busyKey] =
            (target.formVersions[busyKey] ?? 0) + 1;
          if (kind === 'a') target.approvalVersion++;
          else target.clarificationVersion++;
          target.actionError = `This request changed or could not accept that decision: ${message(error)}. Refreshing; review again before submitting.`;
          notify(target);
          await refreshRef.current();
        } else {
          target.actionError = `Could not save: ${message(error)}. Review and retry explicitly.`;
        }
      }
      return false;
    } finally {
      target.busy.delete(busyKey);
      notify(target);
    }
  };

  return {
    available: Boolean(api),
    approvals: store.approvals.filter((item) => item.status === 'pending'),
    clarifications: store.clarifications.filter(
      (item) => item.status === 'pending',
    ),
    canAct: Boolean(
      api && online && (store.approvalsReady || store.clarificationsReady),
    ),
    loading: Boolean(
      api &&
      online &&
      (!store.approvalsReady || !store.clarificationsReady) &&
      !store.approvalError &&
      !store.clarificationError,
    ),
    error: store.actionError ?? store.approvalError ?? store.clarificationError,
    notice: store.notice,
    busyIds: [...store.busy],
    approvalBlocked,
    clarificationBlocked,
    formVersions: store.formVersions,
    refresh: useCallback(() => refreshRef.current(), []),
    onEvent,
    decide: (item, input) => write('a', item, input),
    answer: (item, input) => write('c', item, input),
  };
}

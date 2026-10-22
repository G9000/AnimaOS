import { useState } from 'react';
import type { ApprovalDecision } from '@animaos/sdk';

import type { ApprovalDecide } from '../../lib/approvals';
import type { HashPage } from '../../lib/hash-route';
import {
  approvalHeadline,
  type DoneAction,
  type NeedsYouItem,
} from '../../lib/needs-you';
import { PLAN_TOOL } from '../../lib/plan';
import { XIcon } from '../icons';

export type DoneToday =
  | { state: 'loading' }
  | { state: 'error'; message: string }
  | { state: 'ready'; actions: readonly DoneAction[] };

const KIND_LABEL: Record<NeedsYouItem['kind'], string> = {
  approve: 'Approve',
  pair: 'Approve',
  fix: 'Fix',
  review: 'Review',
};

function timeAgo(ms: number, now = Date.now()): string {
  const minutes = Math.max(0, Math.round((now - ms) / 60_000));
  if (minutes < 1) return 'just now';
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  return new Date(ms).toLocaleDateString([], { month: 'short', day: 'numeric' });
}

function clock(ms: number): string {
  return new Date(ms).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
  });
}

/** The "Needs you" inbox: everything waiting on the owner in one place, and
 *  what the agent did today. Decisions here are the same ones the chat and
 *  Settings offer. */
export function NeedsYouPanel({
  items,
  agentName,
  decideApproval,
  approvePairing,
  openSession,
  openPage,
  done,
  onShowDone,
  onClose,
  onOpenReport,
}: {
  items: readonly NeedsYouItem[];
  agentName: string;
  decideApproval: ApprovalDecide;
  approvePairing: (connectorId: string, chatId: string) => Promise<unknown>;
  openSession: (agentId: string, sessionId: string) => void;
  openPage: (page: HashPage) => void;
  done: DoneToday;
  onShowDone: () => void;
  onClose: () => void;
  onOpenReport?: () => void;
}) {
  const [tab, setTab] = useState<'needs' | 'done'>('needs');
  const [busy, setBusy] = useState<string | null>(null);
  const [errors, setErrors] = useState<Record<string, string>>({});

  const run = async (id: string, action: () => Promise<string | null>) => {
    setBusy(id);
    setErrors(({ [id]: _, ...rest }) => rest);
    const error = await action();
    setBusy(null);
    if (error) setErrors((current) => ({ ...current, [id]: error }));
  };

  const decide = (item: NeedsYouItem & { kind: 'approve' }, decision: ApprovalDecision) =>
    run(item.id, () => decideApproval(item.approval, { decision }));

  const go = (action: () => void) => {
    action();
    onClose();
  };

  const doneCount = done.state === 'ready' ? done.actions.length : null;

  return (
    <div className="needs-you-panel">
      <div className="needs-you-head">
        <div className="needs-you-tabs" role="tablist" aria-label="Inbox">
          <button
            type="button"
            role="tab"
            aria-selected={tab === 'needs'}
            onClick={() => setTab('needs')}
          >
            Needs you · {items.length}
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={tab === 'done'}
            onClick={() => {
              setTab('done');
              onShowDone();
            }}
          >
            Done today{doneCount === null ? '' : ` · ${doneCount}`}
          </button>
        </div>
        <button
          type="button"
          className="studio-tool-button"
          aria-label="Close inbox"
          onClick={onClose}
        >
          <XIcon size={14} />
        </button>
      </div>

      {tab === 'needs' ? (
        items.length === 0 ? (
          <p className="needs-you-empty">
            Nothing needs you right now. {agentName} will put anything that
            does here, and on Telegram if it’s connected.
          </p>
        ) : (
          <ul className="needs-you-list" aria-label="Waiting for you">
            {items.map((item) => (
              <li key={item.id} className="needs-you-item" data-kind={item.kind}>
                <div className="needs-you-meta">
                  <span className="needs-you-kind">{KIND_LABEL[item.kind]}</span>
                  <span>{timeAgo(item.atMs)}</span>
                </div>
                <ItemBody item={item} />
                <div className="needs-you-actions">
                  {item.kind === 'approve' && (
                    <>
                      <button
                        type="button"
                        className="needs-you-primary"
                        disabled={busy === item.id}
                        onClick={() => void decide(item, 'allow_once')}
                      >
                        {item.approval.tool === PLAN_TOOL ? 'Start plan' : 'Allow once'}
                      </button>
                      <button
                        type="button"
                        disabled={busy === item.id}
                        onClick={() => void decide(item, 'deny')}
                      >
                        {item.approval.tool === PLAN_TOOL ? 'Not now' : 'Deny'}
                      </button>
                      <button
                        type="button"
                        onClick={() =>
                          go(() =>
                            openSession(
                              item.approval.agentId,
                              item.approval.sessionId,
                            ),
                          )
                        }
                      >
                        Open chat
                      </button>
                    </>
                  )}
                  {item.kind === 'pair' && (
                    <>
                      <button
                        type="button"
                        className="needs-you-primary"
                        disabled={busy === item.id}
                        onClick={() =>
                          void run(item.id, async () =>
                            (await approvePairing(item.connector.id, item.chatId))
                              ? null
                              : 'Could not approve the chat. Try again from Connections.',
                          )
                        }
                      >
                        Approve chat
                      </button>
                      <button type="button" onClick={() => go(() => openPage('connectors'))}>
                        Open Connections
                      </button>
                    </>
                  )}
                  {item.kind === 'fix' && (
                    <button
                      type="button"
                      className="needs-you-primary"
                      onClick={() => go(() => openPage('connectors'))}
                    >
                      Open Connections
                    </button>
                  )}
                  {item.kind === 'review' && (
                    <button
                      type="button"
                      className="needs-you-primary"
                      onClick={() => go(() => openPage('skills'))}
                    >
                      Review in Skills
                    </button>
                  )}
                </div>
                {errors[item.id] && (
                  <p className="needs-you-error" role="alert">
                    {errors[item.id]}
                  </p>
                )}
              </li>
            ))}
          </ul>
        )
      ) : done.state === 'loading' ? (
        <p className="needs-you-empty" role="status">
          Reading today’s activity…
        </p>
      ) : done.state === 'error' ? (
        <p className="needs-you-error" role="alert">
          {done.message}
        </p>
      ) : done.actions.length === 0 ? (
        <p className="needs-you-empty">
          {agentName} hasn’t changed anything today. Reads and searches aren’t
          listed.
        </p>
      ) : (
        <ul className="needs-you-list" aria-label="Done today">
          {done.actions.map((action) => (
            <li key={action.id} className="needs-you-done" data-failed={action.failed || undefined}>
              <div className="needs-you-done-text">
                <strong>
                  {action.title}
                  {action.failed && <span className="needs-you-failed"> · didn’t work</span>}
                </strong>
                {action.detail && <span>{action.detail}</span>}
              </div>
              <time dateTime={new Date(action.atMs).toISOString()}>
                {clock(action.atMs)}
              </time>
            </li>
          ))}
        </ul>
      )}
      <div className="needs-you-foot">
        <span>
          {tab === 'needs'
            ? 'Approvals here are the same ones shown in the chat.'
            : 'Everything the agent changed today. Reads and searches are left out.'}
        </span>
        {onOpenReport && (
          <button
            type="button"
            onClick={() => {
              onClose();
              onOpenReport();
            }}
          >
            Weekly report
          </button>
        )}
      </div>
    </div>
  );
}

function ItemBody({ item }: { item: NeedsYouItem }) {
  if (item.kind === 'approve') {
    const { title, detail } = approvalHeadline(item.approval);
    return (
      <>
        <strong className="needs-you-title">{title}</strong>
        <span className="needs-you-detail">{detail}</span>
      </>
    );
  }
  if (item.kind === 'pair')
    return (
      <>
        <strong className="needs-you-title">
          {item.chatName} wants to talk on Telegram
        </strong>
        <span className="needs-you-detail">
          Approve it only if it’s you or a group you trust.
        </span>
      </>
    );
  if (item.kind === 'fix')
    return (
      <>
        <strong className="needs-you-title">{item.title}</strong>
        <span className="needs-you-detail">{item.detail}</span>
      </>
    );
  return (
    <>
      <strong className="needs-you-title">A skill to keep or drop</strong>
      <span className="needs-you-detail">{item.suggestion}</span>
    </>
  );
}

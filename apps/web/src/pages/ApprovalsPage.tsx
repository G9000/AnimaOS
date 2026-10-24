import { useState, type FormEvent, type ReactNode } from 'react';
import {
  DEFAULT_APPROVAL_POLICY,
  POLICY_CLASSES,
  type Approval,
  type ApprovalMatcherKind,
  type ApprovalPolicyAction,
  type PolicyClass,
  type Session,
} from '@animaos/sdk';

import { AutonomyCard } from '../components/autonomy/AutonomyCard';
import { CodingKeys } from '../components/coding/CodingKeys';
import { ApprovalCard } from '../components/sessions/ApprovalCard';
import { useApprovals, type ApprovalsView } from '../hooks/useApprovals';
import {
  CLASS_LABELS,
  MATCHER_KIND_LABELS,
  MAX_MATCHER_VALUE_CHARS,
  approvalOutcome,
  decideApproval,
  describeMatcher,
  formatWhen,
  isBroadExecMatcher,
  resolvedAt,
  type ApprovalDecide,
} from '../lib/approvals';
import type { LiveState } from '../lib/session-events';
import { sessionKey } from '../lib/session-groups';

const ACTIONS: readonly ApprovalPolicyAction[] = ['allow', 'ask', 'deny'];
const ACTION_LABELS: Record<ApprovalPolicyAction, string> = {
  allow: 'Allow',
  ask: 'Ask me first',
  deny: 'Deny',
};

const POLICY_ROW_ACTIONS: Record<PolicyClass, string> = {
  write: 'Write files',
  exec: 'Run commands',
  network: 'Read web pages',
  delegate: 'Ask other agents',
};

const POLICY_ROW_WHERE: Record<PolicyClass, string> = {
  write: 'Any path',
  exec: 'Anywhere',
  network: 'Any site',
  delegate: 'Any agent',
};

function ClassRules({
  klass,
  view,
}: {
  klass: PolicyClass;
  view: ApprovalsView;
}) {
  const label = CLASS_LABELS[klass];
  const tools = view.tools.filter((tool) => tool.class === klass);
  const rules = view.rules.filter((rule) =>
    tools.some((tool) => tool.name === rule.tool),
  );
  const [toolName, setToolName] = useState('');
  const [kind, setKind] = useState<ApprovalMatcherKind | null>(null);
  const [value, setValue] = useState('');
  const tool = tools.find((item) => item.name === toolName) ?? tools[0];
  const matcherKind =
    tool && kind && tool.matcherKinds.includes(kind)
      ? kind
      : tool?.matcherKinds[0];

  const broad =
    !!tool &&
    !!matcherKind &&
    isBroadExecMatcher(tool.class, {
      kind: matcherKind,
      value: matcherKind === 'any' ? '' : value.trim(),
    });

  const add = async (event: FormEvent) => {
    event.preventDefault();
    if (!tool || !matcherKind) return;
    const kept = await view.addRule(tool.name, {
      kind: matcherKind,
      value: matcherKind === 'any' ? '' : value.trim(),
    });
    if (kept) setValue('');
  };

  return (
    <>
      <tr className="approvals-policy-row" data-policy-class={klass}>
        <th scope="row">{POLICY_ROW_ACTIONS[klass]}</th>
        <td>{POLICY_ROW_WHERE[klass]}</td>
        <td>
          <div className="approvals-decision-options" role="group" aria-label={`Default decision for ${label}`}>
            {ACTIONS.map((action) => (
              <button
                key={action}
                type="button"
                aria-pressed={(view.policy?.[klass] ?? DEFAULT_APPROVAL_POLICY[klass]) === action}
                disabled={view.policy === null}
                onClick={() => void view.setPolicyAction(klass, action)}
              >
                {action === 'deny' ? 'Block' : ACTION_LABELS[action]}
              </button>
            ))}
          </div>
        </td>
      </tr>
      {rules.map((rule) => {
        const covers = describeMatcher(rule.tool, rule.matcher);
        return (
          <tr key={rule.id} className="approvals-rule-row">
            <th scope="row">{rule.tool}</th>
            <td>{covers}</td>
            <td>
              <span className="approvals-rule-allow">Allow</span>
              <button
                type="button"
                className="approvals-remove-rule"
                aria-label={`Remove rule: ${covers}`}
                onClick={() => void view.removeRule(rule)}
              >
                Remove
              </button>
            </td>
          </tr>
        );
      })}
      {tool && matcherKind && (
        <tr className="approvals-add-rule-row">
          <td colSpan={3}>
            <details className="approvals-add-rule-details">
              <summary>Add rule · {label}</summary>
              <form
                className="approvals-add-rule"
                aria-label={`Add a rule for ${label}`}
                onSubmit={(event) => void add(event)}
              >
                <select
                  aria-label="Tool"
                  value={tool.name}
                  onChange={(event) => setToolName(event.target.value)}
                >
                  {tools.map((item) => (
                    <option key={item.name} value={item.name}>
                      {item.name}
                    </option>
                  ))}
                </select>
                <select
                  aria-label="Match by"
                  value={matcherKind}
                  onChange={(event) =>
                    setKind(event.target.value as ApprovalMatcherKind)
                  }
                >
                  {tool.matcherKinds.map((item) => (
                    <option key={item} value={item}>
                      {MATCHER_KIND_LABELS[item]}
                    </option>
                  ))}
                </select>
                {matcherKind !== 'any' && (
                  <input
                    aria-label="Match value"
                    value={value}
                    maxLength={MAX_MATCHER_VALUE_CHARS}
                    onChange={(event) => setValue(event.target.value)}
                  />
                )}
                <button
                  type="submit"
                  className="studio-tool-button"
                  disabled={matcherKind !== 'any' && !value.trim()}
                >
                  Add rule
                </button>
                {broad && (
                  <p className="approval-card-warning">
                    A rule this broad lets your companion run almost anything,
                    including changing its own approval settings.
                  </p>
                )}
              </form>
            </details>
          </td>
        </tr>
      )}
    </>
  );
}

export interface ApprovalsPageProps {
  agentId: string;
  agentName?: string;
  /** The companion's live stream state. */
  live: LiveState;
  streamOpen: boolean;
  /** The sessions list, for each card's "Open" button. */
  sessions: readonly Session[];
  onOpenSession: (approval: Approval) => void;
  /** What the companion may do at all, shown between what waits now and
   *  the rules for what it must ask. */
  access?: ReactNode;
  /** Bumped when the policy changed elsewhere: read it again. */
  policyRevision?: number;
}

/** Spec §15.4: pending cards, rules and policy per class, and the 30-day
 *  decided history. */
export function ApprovalsPage({
  agentId,
  agentName = 'This agent',
  live,
  streamOpen,
  sessions,
  onOpenSession,
  access = null,
  policyRevision = 0,
}: ApprovalsPageProps) {
  const [section, setSection] = useState<'overview' | 'autonomy' | 'coding'>('overview');
  // An autonomy preset rewrites the class rules, and a class rule may turn
  // the autonomy Custom: each side reads the other again after a change.
  const [presetRevision, setPresetRevision] = useState(0);
  const [classRevision, setClassRevision] = useState(0);
  const approvals = useApprovals({
    agentId,
    streamApprovals: live.approvals,
    streamOpen,
    epoch: live.epoch,
    policyRevision: policyRevision + presetRevision,
  });
  const view: ApprovalsView = {
    ...approvals,
    setPolicyAction: async (klass, action) => {
      await approvals.setPolicyAction(klass, action);
      setClassRevision((value) => value + 1);
    },
  };
  const titles = new Map(
    sessions.map((session) => [sessionKey(session), session.title]),
  );
  const decide: ApprovalDecide = async (approval, input) => {
    const failure = await decideApproval(approval, input);
    // Without the stream nothing else tells this page what became of the
    // approval: a 404 or 409 means it was settled elsewhere.
    if (!streamOpen) view.refresh();
    return failure;
  };

  return (
    <div className="approvals-page">
      {view.error && (
        <p className="approvals-error" role="alert">
          {view.error}
        </p>
      )}
      {section === 'overview' ? (
        <>
          {view.pending.length > 0 && (
            <section className="approvals-notice" aria-live="polite">
              <span>
                {view.pending.length} request{view.pending.length === 1 ? '' : 's'} are waiting in your chats
              </span>
              <button
                type="button"
                className="studio-tool-button"
                onClick={() => onOpenSession(view.pending[0])}
              >
                Show me
              </button>
            </section>
          )}
          {view.pending.length > 0 && (
            <details className="approvals-pending-details">
              <summary>Review requests here</summary>
              <div className="approvals-cards">
                {view.pending.map((approval) => (
                  <ApprovalCard
                    key={approval.id}
                    approval={approval}
                    onDecide={decide}
                    canPersist={approval.agentId === agentId}
                    context={
                      <button
                        type="button"
                        className="studio-tool-button"
                        onClick={() => onOpenSession(approval)}
                      >
                        Open{' '}
                        {titles.get(
                          sessionKey({
                            agentId: approval.agentId,
                            id: approval.sessionId,
                          }),
                        ) ?? 'chat'}
                      </button>
                    }
                  />
                ))}
              </div>
            </details>
          )}
          {access}
          <section className="approvals-section" aria-labelledby="approvals-rules">
            <h2 id="approvals-rules">Rules</h2>
            <p className="approvals-help">
              When one of these runs, {agentName} follows the rule instead of asking
              every time.
            </p>
            <div className="approvals-rules-table-wrap">
              <table className="approvals-rules-table">
                <thead>
                  <tr>
                    <th scope="col">Action</th>
                    <th scope="col">Where</th>
                    <th scope="col">Decision</th>
                  </tr>
                </thead>
                <tbody>
                  {POLICY_CLASSES.map((klass) => (
                    <ClassRules key={klass} klass={klass} view={view} />
                  ))}
                </tbody>
              </table>
            </div>
          </section>
          <details className="approvals-history-details">
            <summary>Decision history</summary>
            {view.decided.length === 0 ? (
              <p className="approvals-empty">No decisions in the last 30 days.</p>
            ) : (
              <ul className="approvals-decided">
                {view.decided.map((approval) => (
                  <li key={approval.id}>
                    <strong>{approval.tool}</strong>
                    <span>{approvalOutcome(approval)}</span>
                    {approval.resolution?.note && (
                      <span className="approvals-note">
                        “{approval.resolution.note}”
                      </span>
                    )}
                    <time dateTime={new Date(resolvedAt(approval)).toISOString()}>
                      {formatWhen(resolvedAt(approval))}
                    </time>
                  </li>
                ))}
              </ul>
            )}
            {view.hasMoreDecided && (
              <button
                type="button"
                className="studio-tool-button"
                onClick={view.loadMoreDecided}
              >
                Show older
              </button>
            )}
          </details>
          <details className="approvals-more-settings">
            <summary>More permission settings</summary>
            <div className="approvals-shortcuts" aria-label="More permission settings">
              <button type="button" onClick={() => setSection('autonomy')}>
                When it checks with you <span aria-hidden="true">→</span>
              </button>
              <button type="button" onClick={() => setSection('coding')}>
                Coding app keys <span aria-hidden="true">→</span>
              </button>
            </div>
          </details>
        </>
      ) : (
        <>
          <button
            type="button"
            className="approvals-back"
            onClick={() => setSection('overview')}
          >
            <span aria-hidden="true">←</span> Permissions
          </button>
          {section === 'autonomy' ? (
            <AutonomyCard
              agentId={agentId}
              revision={policyRevision + classRevision}
              onModeChanged={() => setPresetRevision((value) => value + 1)}
            />
          ) : (
            <>
              <h2 className="approvals-detail-title">Keys for coding apps</h2>
              {access}
              <CodingKeys agentId={agentId} />
            </>
          )}
        </>
      )}
    </div>
  );
}

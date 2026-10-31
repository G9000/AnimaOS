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
import {
  SettingsCard,
  SettingsEmpty,
  SettingsRow,
  SettingsSection,
} from '../components/settings/SettingsKit';
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

/** Recent decisions show this many at a time. */
const HISTORY_PAGE = 5;

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
  const [adding, setAdding] = useState(false);
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

  const decision = view.policy?.[klass] ?? DEFAULT_APPROVAL_POLICY[klass];
  return (
    <div className="sk-row approvals-class" data-policy-class={klass}>
      <div className="sk-row-main">
        <div className="sk-row-text">
          <span className="sk-row-title">{POLICY_ROW_ACTIONS[klass]}</span>
          {rules.length > 0 && (
            <span className="sk-row-desc">
              {rules.length} {rules.length === 1 ? 'exception' : 'exceptions'}{' '}
              always allowed
            </span>
          )}
        </div>
        <div className="sk-row-control">
          {tool && matcherKind && (
            <button
              type="button"
              className="approvals-add-exception"
              aria-label={`Add an exception for ${label}`}
              title="Always allow something specific"
              aria-expanded={adding}
              onClick={() => setAdding((open) => !open)}
            >
              {adding ? '−' : '+'}
            </button>
          )}
          <div
            className="sk-segmented"
            role="group"
            aria-label={`Default decision for ${label}`}
          >
            {ACTIONS.map((action) => (
              <button
                key={action}
                type="button"
                data-tone={action}
                aria-pressed={decision === action}
                disabled={view.policy === null}
                onClick={() => void view.setPolicyAction(klass, action)}
              >
                {action === 'deny' ? 'Block' : ACTION_LABELS[action]}
              </button>
            ))}
          </div>
        </div>
      </div>
      {rules.length > 0 && (
        <ul className="approvals-rule-list">
          {rules.map((rule) => {
            const covers = describeMatcher(rule.tool, rule.matcher);
            return (
              <li key={rule.id}>
                <code>{rule.tool}</code>
                <span className="approvals-rule-covers">{covers}</span>
                <span className="sk-badge">Allow</span>
                <button
                  type="button"
                  className="ui-button is-quiet"
                  aria-label={`Remove rule: ${covers}`}
                  onClick={() => void view.removeRule(rule)}
                >
                  Remove
                </button>
              </li>
            );
          })}
        </ul>
      )}
      {tool && matcherKind && adding && (
        <form
          className="approvals-add-rule"
          aria-label={`Add a rule for ${label}`}
          onSubmit={(event) => void add(event)}
        >
          <select
            aria-label="Tool"
            className="field"
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
            className="field"
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
              className="field"
              value={value}
              maxLength={MAX_MATCHER_VALUE_CHARS}
              onChange={(event) => setValue(event.target.value)}
            />
          )}
          <button
            type="submit"
            className="ui-button"
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
      )}
    </div>
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
  const [section, setSection] = useState<'overview' | 'autonomy' | 'coding'>(
    'overview',
  );
  const [historyShown, setHistoryShown] = useState(HISTORY_PAGE);
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
    <div className="approvals-page" data-section={section}>
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
                {view.pending.length === 1
                  ? '1 request is waiting in your chats'
                  : `${view.pending.length} requests are waiting in your chats`}
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
          <SettingsSection
            id="approvals-rules"
            title="What needs your OK"
            description={`For each kind of action, ${agentName} goes ahead, asks you first, or is blocked. Use + to always allow something specific.`}
          >
            <SettingsCard className="approvals-rules">
              {POLICY_CLASSES.map((klass) => (
                <ClassRules key={klass} klass={klass} view={view} />
              ))}
            </SettingsCard>
          </SettingsSection>
          <SettingsSection
            id="approvals-more"
            title="More permission settings"
            description="How often it checks in, and keys for the coding apps it runs."
          >
            <SettingsCard className="approvals-shortcuts">
              <button
                type="button"
                className="sk-row sk-nav"
                onClick={() => setSection('autonomy')}
              >
                <span className="sk-row-text">
                  <span className="sk-row-title">When it checks with you</span>
                  <span className="sk-row-desc">
                    Before every change, only for risky things, or rarely.
                  </span>
                </span>
                <span aria-hidden="true">→</span>
              </button>
              <button
                type="button"
                className="sk-row sk-nav"
                onClick={() => setSection('coding')}
              >
                <span className="sk-row-text">
                  <span className="sk-row-title">Coding app keys</span>
                  <span className="sk-row-desc">
                    Keys the coding apps it runs can use.
                  </span>
                </span>
                <span aria-hidden="true">→</span>
              </button>
            </SettingsCard>
          </SettingsSection>
          <SettingsSection
            id="approvals-history"
            className="approvals-history"
            title="Recent decisions"
            description="What you allowed or blocked in the last 30 days, so you can check what it was given."
          >
            <SettingsCard as="ul" className="approvals-decided">
              {view.decided.length === 0 ? (
                <li>
                  <SettingsEmpty>
                    No decisions in the last 30 days.
                  </SettingsEmpty>
                </li>
              ) : (
                view.decided.slice(0, historyShown).map((approval) => (
                  <SettingsRow
                    as="li"
                    key={approval.id}
                    title={<code>{approval.tool}</code>}
                    description={
                      <>
                        {approvalOutcome(approval)}
                        {approval.resolution?.note && (
                          <span className="approvals-note">
                            {' '}
                            “{approval.resolution.note}”
                          </span>
                        )}
                      </>
                    }
                    control={
                      <time
                        className="sk-meta"
                        dateTime={new Date(resolvedAt(approval)).toISOString()}
                      >
                        {formatWhen(resolvedAt(approval))}
                      </time>
                    }
                  />
                ))
              )}
            </SettingsCard>
            {(view.decided.length > historyShown || view.hasMoreDecided) && (
              <button
                type="button"
                className="ui-button is-quiet approvals-history-more"
                onClick={() => {
                  if (view.decided.length > historyShown)
                    setHistoryShown((shown) => shown + HISTORY_PAGE);
                  else view.loadMoreDecided();
                }}
              >
                Show older
              </button>
            )}
          </SettingsSection>
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

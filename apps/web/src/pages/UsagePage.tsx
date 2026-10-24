import { useEffect, useState, type FormEvent, type ReactNode } from 'react';
import type { UsageGroup, UsageTotals } from '@animaos/sdk';

import { UsageChart } from '../components/usage/UsageChart';
import { UsageTable } from '../components/usage/UsageTable';
import { useSessionUsage } from '../hooks/useSessionUsage';
import { useUsage } from '../hooks/useUsage';
import { COMPANION_UNREACHABLE } from '../lib/approvals';
import { daemon } from '../lib/daemon-api';
import {
  DEFAULT_USAGE_RANGE_DAYS,
  USAGE_EMPTY,
  USAGE_RANGES_DAYS,
  USAGE_TRUNCATED_NOTE,
  costNote,
  formatCost,
  formatTokens,
  sourceLabel,
  subscriptionNote,
  type UsageRangeDays,
} from '../lib/usage';

export interface UsagePageProps {
  agentId: string | null;
  online: boolean;
  /** `LiveState.epoch`: bumped by every snapshot and resync. */
  epoch: number;
  /** The session last viewed, for the This chat card in agent-scoped views. */
  sessionId: string | null;
  /** The Settings design shows usage across every agent. */
  allAgents?: boolean;
  agents?: readonly { id: string; name: string }[];
}

const RANGE_STORAGE_KEY = 'anima.usage.range';
const ALL_AGENTS_RANGE_STORAGE_KEY = 'anima.usage.all-agents.range';
const LOADING_TEXT = 'Loading usage…';
const NO_SESSION_LABEL = 'No chat';
const NO_AGENTS: readonly { id: string; name: string }[] = [];

function calendarMonthRanges(now: Date) {
  const currentStart = new Date(now.getFullYear(), now.getMonth(), 1).getTime();
  const previousStart = new Date(
    now.getFullYear(),
    now.getMonth() - 1,
    1,
  ).getTime();
  return {
    current: { from: currentStart, to: now.getTime() + 1 },
    previous: { from: previousStart, to: currentStart },
    tzOffsetMinutes: -now.getTimezoneOffset(),
  };
}

function monthComparison(current: number, previous: number): string {
  if (previous <= 0) return 'Compared with last month';
  const change = Math.round(((current - previous) / previous) * 100);
  const direction = change < 0 ? '↓' : change > 0 ? '↑' : '·';
  return `${direction} ${Math.abs(change)}% vs last month`;
}

function readRange(): UsageRangeDays {
  try {
    const stored = Number(sessionStorage.getItem(RANGE_STORAGE_KEY));
    const found = USAGE_RANGES_DAYS.find((days) => days === stored);
    if (found) return found;
  } catch {
    // Storage can be blocked; the page works without it.
  }
  return DEFAULT_USAGE_RANGE_DAYS;
}

function rememberRange(days: UsageRangeDays) {
  try {
    sessionStorage.setItem(RANGE_STORAGE_KEY, String(days));
  } catch {
    // Not remembered; nothing else depends on it.
  }
}

function readAllAgentsRange(): UsageRangeDays {
  try {
    const stored = Number(sessionStorage.getItem(ALL_AGENTS_RANGE_STORAGE_KEY));
    const found = USAGE_RANGES_DAYS.find((days) => days === stored);
    if (found) return found;
  } catch {
    // Storage can be blocked; the page works without it.
  }
  return 14;
}

function rememberAllAgentsRange(days: UsageRangeDays) {
  try {
    sessionStorage.setItem(ALL_AGENTS_RANGE_STORAGE_KEY, String(days));
  } catch {
    // Not remembered; nothing else depends on it.
  }
}

function Card({
  label,
  value,
  children,
}: {
  label: string;
  value: string;
  children?: ReactNode;
}) {
  return (
    <div className="usage-card" role="group" aria-label={label}>
      <span className="usage-card-label">{label}</span>
      <strong className="usage-card-value">{value}</strong>
      {children}
    </div>
  );
}

/** Spec §15.4: what the companion costs. Model, provider, and session names
 *  render as text. */
export function UsagePage(props: UsagePageProps) {
  if (props.allAgents) {
    return (
      <AllAgentsUsagePage
        agents={props.agents ?? NO_AGENTS}
        online={props.online}
        epoch={props.epoch}
      />
    );
  }
  return <SingleAgentUsagePage {...props} />;
}

function AllAgentsUsagePage({
  agents,
  online,
  epoch,
}: {
  agents: readonly { id: string; name: string }[];
  online: boolean;
  epoch: number;
}) {
  const [days, setDays] = useState<UsageRangeDays>(readAllAgentsRange);
  const [exporting, setExporting] = useState(false);
  const [monthSummary, setMonthSummary] = useState<{
    epoch: number;
    current: UsageTotals | null;
    previous: UsageTotals | null;
  } | null>(null);
  const [monthlyLimitState, setMonthlyLimitState] = useState<{
    epoch: number;
    limitMicros: number | null;
    error: string | null;
  } | null>(null);
  const [editingLimit, setEditingLimit] = useState(false);
  const [limitDraft, setLimitDraft] = useState('');
  const [savingLimit, setSavingLimit] = useState(false);
  const [limitError, setLimitError] = useState<string | null>(null);
  const [agentRows, setAgentRows] = useState<UsageGroup[]>([]);
  const [sessionTitles, setSessionTitles] = useState<Record<string, string>>(
    {},
  );
  const [sessionAgentCounts, setSessionAgentCounts] = useState<
    Record<string, number>
  >({});
  const view = useUsage({
    enabled: online,
    days,
    agentId: null,
    allAgents: true,
    epoch,
  });
  const monthLoaded = monthSummary?.epoch === epoch;
  const monthTotals = monthLoaded ? monthSummary?.current ?? null : null;
  const previousMonthTotals = monthLoaded ? monthSummary?.previous ?? null : null;
  const limitLoaded = monthlyLimitState?.epoch === epoch;
  const monthlyLimit = limitLoaded
    ? (monthlyLimitState?.limitMicros ?? null)
    : null;
  const { from, to, tzOffsetMinutes } = view.range;

  useEffect(() => {
    if (!online) return;
    let cancelled = false;
    const ranges = calendarMonthRanges(new Date());
    void Promise.allSettled([
      daemon.usageSummary({
        ...ranges.current,
        tzOffsetMinutes: ranges.tzOffsetMinutes,
      }),
      daemon.usageSummary({
        ...ranges.previous,
        tzOffsetMinutes: ranges.tzOffsetMinutes,
      }),
    ]).then(([current, previous]) => {
      if (cancelled) return;
      setMonthSummary({
        epoch,
        current: current.status === 'fulfilled' ? current.value.totals : null,
        previous:
          previous.status === 'fulfilled' ? previous.value.totals : null,
      });
    });
    return () => {
      cancelled = true;
    };
  }, [online, epoch]);

  useEffect(() => {
    if (!online) return;
    let cancelled = false;
    void daemon.monthlySpendLimit().then(
      (response) => {
        if (cancelled) return;
        setMonthlyLimitState({
          epoch,
          limitMicros: response.limitMicros,
          error: null,
        });
      },
      (error: unknown) => {
        if (cancelled) return;
        setMonthlyLimitState({
          epoch,
          limitMicros: null,
          error: error instanceof Error ? error.message : 'Could not load the monthly limit.',
        });
      },
    );
    return () => {
      cancelled = true;
    };
  }, [online, epoch]);

  useEffect(() => {
    if (!online || !view.loaded) return;
    let cancelled = false;
    void Promise.allSettled(
      agents.map(async (agent) => {
        const [summary, active, archived] = await Promise.all([
          daemon.usageSummary({
            from,
            to,
            agentId: agent.id,
            groupBy: 'session',
            tzOffsetMinutes,
          }),
          daemon.listSessions(agent.id, { limit: 200 }),
          daemon.listSessions(agent.id, { limit: 200, archived: true }),
        ]);
        return {
          agent,
          summary,
          sessions: [...active.sessions, ...archived.sessions],
        };
      }),
    ).then((results) => {
      if (cancelled) return;
      const rows: UsageGroup[] = [];
      const titles = new Map<string, string>();
      const contributors = new Map<string, Set<string>>();
      for (const result of results) {
        if (result.status !== 'fulfilled') continue;
        const { agent, summary, sessions } = result.value;
        if (summary.totals.calls > 0)
          rows.push({ key: agent.name, totals: summary.totals });
        for (const group of summary.groups) {
          if (!group.key) continue;
          const ids = contributors.get(group.key) ?? new Set<string>();
          ids.add(agent.id);
          contributors.set(group.key, ids);
        }
        for (const session of sessions) {
          const title = session.title?.trim();
          if (title) titles.set(session.id, title);
        }
      }
      rows.sort((left, right) => right.totals.costMicros - left.totals.costMicros);
      setAgentRows(rows);
      setSessionTitles(Object.fromEntries(titles));
      setSessionAgentCounts(
        Object.fromEntries(
          [...contributors].map(([id, owners]) => [id, owners.size]),
        ),
      );
    });
    return () => {
      cancelled = true;
    };
  }, [
    agents,
    online,
    view.loaded,
    from,
    to,
    tzOffsetMinutes,
  ]);

  if (!online) {
    return (
      <div className="usage-page">
        <p className="usage-note" role="status">
          {COMPANION_UNREACHABLE}
        </p>
      </div>
    );
  }

  const chooseRange = (next: UsageRangeDays) => {
    setDays(next);
    rememberAllAgentsRange(next);
  };
  const exportCsv = async () => {
    setExporting(true);
    try {
      await view.exportCsv();
    } finally {
      setExporting(false);
    }
  };

  const saveMonthlyLimit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const amount = Number(limitDraft);
    if (!Number.isFinite(amount) || amount < 0.01 || amount > 1_000_000) {
      setLimitError('Enter an amount from $0.01 to $1,000,000.');
      return;
    }
    setSavingLimit(true);
    setLimitError(null);
    try {
      const response = await daemon.setMonthlySpendLimit(
        Math.round(amount * 1_000_000),
      );
      setMonthlyLimitState({ epoch, limitMicros: response.limitMicros, error: null });
      setEditingLimit(false);
    } catch (error) {
      setLimitError(error instanceof Error ? error.message : 'Could not save the monthly limit.');
    } finally {
      setSavingLimit(false);
    }
  };

  const clearMonthlyLimit = async () => {
    setSavingLimit(true);
    setLimitError(null);
    try {
      const response = await daemon.setMonthlySpendLimit(null);
      setMonthlyLimitState({ epoch, limitMicros: response.limitMicros, error: null });
      setEditingLimit(false);
    } catch (error) {
      setLimitError(error instanceof Error ? error.message : 'Could not remove the monthly limit.');
    } finally {
      setSavingLimit(false);
    }
  };

  const { totals } = view;
  const busiestChat = [...view.sessions].sort(
    (left, right) => right.totals.totalTokens - left.totals.totalTokens,
  )[0];
  const busiestChatName = busiestChat
    ? sessionTitles[busiestChat.key] || busiestChat.key || NO_SESSION_LABEL
    : 'No chat yet';
  const busiestChatAgents = busiestChat
    ? sessionAgentCounts[busiestChat.key] ?? 0
    : 0;
  const gone = view.errorStatus === 404;
  const empty = totals !== null && totals.calls === 0;

  return (
    <div className="usage-page">
      <div className="usage-header">
        <h2 className="sr-only">What all agents cost</h2>
        <label className="usage-range-select">
          <span className="sr-only">Usage range</span>
          <select
            className="studio-tool-button"
            aria-label="Usage range"
            value={days}
            onChange={(event) =>
              chooseRange(Number(event.target.value) as UsageRangeDays)
            }
          >
            {USAGE_RANGES_DAYS.map((option) => (
              <option key={option} value={option}>
                Last {option} days
              </option>
            ))}
          </select>
        </label>
        {!gone && (
          <button
            type="button"
            className="studio-tool-button"
            disabled={exporting || !view.loaded}
            onClick={() => void exportCsv()}
          >
            Export CSV
          </button>
        )}
      </div>
      {view.error && (
        <div className="usage-header">
          <p className="usage-error" role="alert">
            {view.error}
          </p>
          <button
            type="button"
            className="studio-tool-button"
            onClick={view.refresh}
          >
            Refresh
          </button>
        </div>
      )}
      {!view.loaded && !view.error && (
        <p className="usage-note" role="status">
          {LOADING_TEXT}
        </p>
      )}
      {!gone && totals && (
        <>
          <div className="usage-cards">
            <Card
              label="This month"
              value={
                monthTotals
                  ? formatCost(monthTotals.costMicros)
                  : monthLoaded
                    ? 'Unavailable'
                    : LOADING_TEXT
              }
            >
              {monthTotals && previousMonthTotals && (
                <small>
                  {monthComparison(
                    monthTotals.costMicros,
                    previousMonthTotals.costMicros,
                  )}
                </small>
              )}
              {monthTotals && costNote(monthTotals) && (
                <small>{costNote(monthTotals)}</small>
              )}
            </Card>
            <Card label="Tokens" value={formatTokens(totals.totalTokens)}>
              <small>
                {formatTokens(totals.promptTokens)} in ·{' '}
                {formatTokens(totals.completionTokens)} out
              </small>
            </Card>
            <Card label="Busiest chat" value={busiestChatName}>
              {busiestChat && (
                <small>
                  {formatCost(busiestChat.totals.costMicros)}
                  {busiestChatAgents > 0 && ` · ${busiestChatAgents} agents`}
                </small>
              )}
            </Card>
            <Card
              label="Limit"
              value={
                !limitLoaded
                  ? LOADING_TEXT
                  : monthlyLimit === null
                    ? 'Not set'
                    : `${formatCost(monthlyLimit)} / month`
              }
            >
              <small>
                {!limitLoaded
                  ? 'Loading the monthly limit…'
                  : monthlyLimit === null
                    ? (monthlyLimitState?.error ?? 'Add a monthly limit to pause new runs at 90%.')
                    : monthTotals
                      ? `${Math.floor((monthTotals.costMicros / monthlyLimit) * 100)}% used · pauses and asks at 90%`
                      : 'Usage for this month is still loading.'}
              </small>
              <button
                type="button"
                className="usage-limit-edit"
                disabled={!limitLoaded || savingLimit}
                onClick={() => {
                  setLimitDraft(
                    monthlyLimit === null
                      ? ''
                      : (monthlyLimit / 1_000_000).toFixed(2),
                  );
                  setLimitError(null);
                  setEditingLimit((value) => !value);
                }}
              >
                {editingLimit ? 'Cancel' : monthlyLimit === null ? 'Set limit' : 'Change'}
              </button>
              {editingLimit && (
                <form className="usage-limit-form" onSubmit={(event) => void saveMonthlyLimit(event)}>
                  <label>
                    <span>Monthly limit (USD)</span>
                    <input
                      type="number"
                      min="0.01"
                      max="1000000"
                      step="0.01"
                      required
                      value={limitDraft}
                      onChange={(event) => setLimitDraft(event.target.value)}
                      disabled={savingLimit}
                    />
                  </label>
                  <button type="submit" disabled={savingLimit}>
                    {savingLimit ? 'Saving…' : 'Save limit'}
                  </button>
                  {monthlyLimit !== null && (
                    <button type="button" disabled={savingLimit} onClick={() => void clearMonthlyLimit()}>
                      Remove limit
                    </button>
                  )}
                  {limitError && <p role="alert">{limitError}</p>}
                </form>
              )}
            </Card>
          </div>
          {view.truncated && (
            <p className="usage-note" role="status">
              {USAGE_TRUNCATED_NOTE}
            </p>
          )}
          {empty ? (
            <p className="usage-note">{USAGE_EMPTY}</p>
          ) : (
            <>
              <section className="usage-chart-section" aria-label="Per day">
                <h3>Per day</h3>
                <UsageChart days={view.days} rangeDays={days} />
              </section>
              <UsageTable
                title="By chat"
                keyHeading="Chat"
                rows={view.sessions}
                renderKey={(key) =>
                  key === ''
                    ? NO_SESSION_LABEL
                    : `${sessionTitles[key] || key}${sessionAgentCounts[key] ? ` · ${sessionAgentCounts[key]} agents` : ''}`
                }
              />
              <details className="usage-breakdowns">
                <summary>Other breakdowns</summary>
                <UsageTable
                  title="By agent"
                  keyHeading="Agent"
                  rows={agentRows}
                />
                <UsageTable
                  title="By model"
                  keyHeading="Model"
                  rows={view.models}
                />
                <UsageTable
                  title="By source"
                  keyHeading="Source"
                  rows={view.sources}
                  renderKey={sourceLabel}
                />
              </details>
            </>
          )}
        </>
      )}
    </div>
  );
}

function SingleAgentUsagePage({
  agentId,
  online,
  epoch,
  sessionId,
}: UsagePageProps) {
  const [days, setDays] = useState<UsageRangeDays>(readRange);
  const [exporting, setExporting] = useState(false);
  const view = useUsage({ enabled: online, days, agentId, epoch });
  const chat = useSessionUsage(
    online && agentId && sessionId ? { agentId, sessionId } : null,
    epoch,
  );

  if (!online) {
    return (
      <div className="usage-page">
        <p className="usage-note" role="status">
          {COMPANION_UNREACHABLE}
        </p>
      </div>
    );
  }

  const chooseRange = (next: UsageRangeDays) => {
    setDays(next);
    rememberRange(next);
  };
  const exportCsv = async () => {
    setExporting(true);
    try {
      await view.exportCsv();
    } finally {
      setExporting(false);
    }
  };

  const { totals } = view;
  const busiestChat = [...view.sessions].sort(
    (left, right) => right.totals.totalTokens - left.totals.totalTokens,
  )[0];
  const gone = view.errorStatus === 404;
  const empty = totals !== null && totals.calls === 0;

  return (
    <div className="usage-page">
      <div className="usage-header">
        <h2 className="sr-only">What your companion costs</h2>
        <div className="usage-ranges" role="group" aria-label="Range">
          {USAGE_RANGES_DAYS.map((option) => (
            <button
              key={option}
              type="button"
              className="studio-tool-button"
              aria-pressed={days === option}
              onClick={() => chooseRange(option)}
            >
              {option} days
            </button>
          ))}
        </div>
        {!gone && (
          <button
            type="button"
            className="studio-tool-button"
            disabled={exporting || !view.loaded}
            onClick={() => void exportCsv()}
          >
            Export CSV
          </button>
        )}
      </div>
      {view.error && (
        <div className="usage-header">
          <p className="usage-error" role="alert">
            {view.error}
          </p>
          <button
            type="button"
            className="studio-tool-button"
            onClick={view.refresh}
          >
            Refresh
          </button>
        </div>
      )}
      {!view.loaded && !view.error && (
        <p className="usage-note" role="status">
          {LOADING_TEXT}
        </p>
      )}
      {!gone && totals && (
        <>
          <div className="usage-cards">
            <Card label="Cost" value={formatCost(totals.costMicros)}>
              {costNote(totals) && <small>{costNote(totals)}</small>}
              {subscriptionNote(totals) && (
                <small>{subscriptionNote(totals)}</small>
              )}
            </Card>
            <Card label="Tokens" value={formatTokens(totals.totalTokens)}>
              <small>
                {formatTokens(totals.promptTokens)} in ·{' '}
                {formatTokens(totals.completionTokens)} out
              </small>
            </Card>
            <Card
              label="Busiest chat"
              value={busiestChat?.key || 'No chat yet'}
            >
              {busiestChat && (
                <small>
                  {formatTokens(busiestChat.totals.totalTokens)} tokens ·{' '}
                  {formatCost(busiestChat.totals.costMicros)}
                </small>
              )}
            </Card>
            <Card label="Calls" value={String(totals.calls)} />
            {sessionId && chat && (
              <Card label="This chat" value={formatTokens(chat.totalTokens)}>
                <small>{formatCost(chat.costMicros)}</small>
              </Card>
            )}
          </div>
          {view.truncated && (
            <p className="usage-note" role="status">
              {USAGE_TRUNCATED_NOTE}
            </p>
          )}
          {empty ? (
            <p className="usage-note">{USAGE_EMPTY}</p>
          ) : (
            <>
              <section className="usage-chart-section" aria-label="Per day">
                <h3>Per day</h3>
                <UsageChart days={view.days} rangeDays={days} />
              </section>
              <UsageTable
                title="By chat"
                keyHeading="Chat"
                rows={view.sessions}
                renderKey={(key) =>
                  key === '' ? (
                    NO_SESSION_LABEL
                  ) : (
                    <a href={`#/s/${encodeURIComponent(key)}`}>{key}</a>
                  )
                }
              />
              <details className="usage-breakdowns">
                <summary>Other breakdowns</summary>
                <UsageTable
                  title="By model"
                  keyHeading="Model"
                  rows={view.models}
                />
                <UsageTable
                  title="By source"
                  keyHeading="Source"
                  rows={view.sources}
                  renderKey={sourceLabel}
                />
              </details>
            </>
          )}
        </>
      )}
    </div>
  );
}

import type { AgentMemory, Run, UsageSummary } from '@animaOS-SWARM/sdk';

import { doneActions } from './needs-you';

export const REPORT_DAYS = 7;
const DAY_MS = 86_400_000;

export interface WeeklyReport {
  from: number;
  to: number;
  /** Replies and tasks that finished. */
  finished: number;
  /** Runs that ended in an error, newest first. */
  failed: { runId: string; sessionId: string; atMs: number; reason: string }[];
  /** Things the agent changed, by kind, most first. */
  highlights: { title: string; count: number }[];
  actions: number;
  /** Priced model spend, in micro-USD, and calls that couldn't be priced. */
  spentMicros: number;
  unpricedCalls: number;
  /** Spend per day, oldest first, one entry for each of the seven days. */
  days: { label: string; micros: number }[];
  /** Memories saved this week, newest first. */
  learned: AgentMemory[];
}

/** The week that ends now: midnight six days ago until now. */
export function reportWindow(now = new Date()): { from: number; to: number } {
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  return {
    from: today.getTime() - (REPORT_DAYS - 1) * DAY_MS,
    to: now.getTime(),
  };
}

/** What happened this week, from the daemon's own records: nothing here is
 *  estimated or invented. */
export function buildWeeklyReport({
  runs,
  usage,
  memories,
  from,
  to,
}: {
  runs: readonly Run[];
  usage: UsageSummary | null;
  memories: readonly AgentMemory[];
  from: number;
  to: number;
}): WeeklyReport {
  const inWeek = (ms: number) => ms >= from && ms < to;
  const weekRuns = runs.filter((run) =>
    inWeek(run.finishedAtMs ?? run.createdAtMs),
  );
  const failed = weekRuns
    .filter((run) => run.status === 'failed')
    .map((run) => ({
      runId: run.id,
      sessionId: run.sessionId,
      atMs: run.finishedAtMs ?? run.createdAtMs,
      reason: run.error?.message ?? 'It stopped with an error.',
    }))
    .sort((left, right) => right.atMs - left.atMs);
  const actions = doneActions(weekRuns).filter(
    (action) => !action.failed && inWeek(action.atMs),
  );
  const counts = new Map<string, number>();
  for (const action of actions)
    counts.set(action.title, (counts.get(action.title) ?? 0) + 1);
  const highlights = [...counts]
    .map(([title, count]) => ({ title, count }))
    .sort((left, right) => right.count - left.count)
    .slice(0, 5);

  const byDay = new Map<string, number>(
    (usage?.groups ?? []).map((group) => [group.key, group.totals.costMicros]),
  );
  const days = Array.from({ length: REPORT_DAYS }, (_, index) => {
    const day = new Date(from + index * DAY_MS);
    return {
      label: day.toLocaleDateString([], { weekday: 'narrow' }),
      micros: byDay.get(dayKey(day)) ?? 0,
    };
  });

  return {
    from,
    to,
    finished: weekRuns.filter((run) => run.status === 'completed').length,
    failed,
    highlights,
    actions: actions.length,
    spentMicros: usage?.totals.costMicros ?? 0,
    unpricedCalls: usage?.totals.unpricedCalls ?? 0,
    days,
    learned: memories
      .filter((memory) => inWeek(memory.createdAt))
      .sort((left, right) => right.createdAt - left.createdAt),
  };
}

/** The daemon's day key for a `groupBy: day` summary (YYYY-MM-DD). */
function dayKey(day: Date): string {
  const month = String(day.getMonth() + 1).padStart(2, '0');
  const date = String(day.getDate()).padStart(2, '0');
  return `${day.getFullYear()}-${month}-${date}`;
}

export function formatMicros(micros: number): string {
  const dollars = micros / 1_000_000;
  return dollars >= 10
    ? `$${dollars.toFixed(0)}`
    : dollars >= 0.01 || dollars === 0
      ? `$${dollars.toFixed(2)}`
      : '< $0.01';
}

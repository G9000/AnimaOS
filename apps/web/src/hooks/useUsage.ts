import { useCallback, useEffect, useRef, useState } from 'react';
import {
  DaemonHttpError,
  type UsageGroup,
  type UsageGroupBy,
  type UsageSummary,
  type UsageTotals,
} from '@animaos/sdk';

import { daemon } from '../lib/daemon-api';
import { useLatest } from './useLatest';
import { useRenderScope } from './useRenderScope';
import { downloadText } from '../lib/download';
import {
  USAGE_EXPORT_FAILED,
  fillDays,
  usageCsvFilename,
  usageErrorMessage,
  usageRange,
  type UsageRange,
  type UsageRangeDays,
} from '../lib/usage';

export interface UsageOptions {
  /** False while the daemon is offline: nothing is read. */
  enabled: boolean;
  days: UsageRangeDays;
  agentId: string | null;
  /** Include usage from every agent instead of filtering to `agentId`. */
  allAgents?: boolean;
  /** `LiveState.epoch`: bumped by every snapshot and resync. */
  epoch: number;
  /** The clock; tests fix it. */
  now?: () => Date;
}

export interface UsageView {
  range: UsageRange;
  /** Every day of the range, ascending. */
  days: UsageGroup[];
  models: UsageGroup[];
  sources: UsageGroup[];
  sessions: UsageGroup[];
  totals: UsageTotals | null;
  /** The range's last day. */
  today: UsageTotals | null;
  truncated: boolean;
  loaded: boolean;
  error: string | null;
  /** The HTTP status of the failed read, when the daemon refused it. */
  errorStatus: number | null;
  refresh: () => void;
  /** Reads the range's CSV and downloads it; true when it did. */
  exportCsv: () => Promise<boolean>;
}

const GROUPS: readonly UsageGroupBy[] = ['day', 'model', 'source', 'session'];

/** A list that did not change keeps its array, so a harness that goes quiet
 *  does not render again (the empty reload included). */
function keepSame<T>(previous: T, next: T): T {
  return JSON.stringify(previous) === JSON.stringify(next) ? previous : next;
}

function rangeFor(days: number, now: () => Date): UsageRange {
  return usageRange(days, now());
}

const NO_GROUPS: UsageGroup[] = [];

/** The Usage page's data (spec §15.4): four summaries over the range. */
export function useUsage({
  enabled,
  days,
  agentId,
  allAgents = false,
  epoch,
  now = () => new Date(),
}: UsageOptions): UsageView {
  const clock = useLatest(now);
  const [range, setRange] = useState(() => rangeFor(days, now));
  const [dayList, setDayList] = useState<UsageGroup[]>(NO_GROUPS);
  const [models, setModels] = useState<UsageGroup[]>(NO_GROUPS);
  const [sources, setSources] = useState<UsageGroup[]>(NO_GROUPS);
  const [sessions, setSessions] = useState<UsageGroup[]>(NO_GROUPS);
  const [totals, setTotals] = useState<UsageTotals | null>(null);
  const [truncated, setTruncated] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [readError, setReadError] = useState<{
    message: string;
    status: number | null;
  } | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);
  const scope = useRenderScope(enabled, allAgents, agentId, days, epoch);
  const [previousScope, setPreviousScope] = useState(scope);
  if (previousScope !== scope) {
    setPreviousScope(scope);
    setExportError(null);
  }
  // The newest read wins; a late answer to an older one is dropped.
  const sequence = useRef(0);
  const unmounted = useRef(false);
  const daysRef = useLatest(days);
  const agentRef = useLatest(agentId);

  const load = useCallback(async () => {
    if (!agentId && !allAgents) return;
    const mine = ++sequence.current;
    const next = rangeFor(days, clock.current);
    const query = {
      from: next.from,
      to: next.to,
      ...(allAgents ? {} : { agentId: agentId! }),
      tzOffsetMinutes: next.tzOffsetMinutes,
    };
    const settled = await Promise.allSettled(
      GROUPS.map((groupBy) => daemon.usageSummary({ ...query, groupBy })),
    );
    if (unmounted.current || mine !== sequence.current) return;

    const [day, model, source, session] = settled;
    const value = (result: PromiseSettledResult<UsageSummary>) =>
      result.status === 'fulfilled' ? result.value : null;
    const failure = settled.find((result) => result.status === 'rejected');

    setRange((previous) => keepSame(previous, next));
    const daySummary = value(day);
    if (daySummary) {
      setDayList((previous) =>
        keepSame(previous, fillDays(daySummary.groups, next)),
      );
      setTotals((previous) => keepSame(previous, daySummary.totals));
    }
    const modelSummary = value(model);
    if (modelSummary)
      setModels((previous) => keepSame(previous, modelSummary.groups));
    const sourceSummary = value(source);
    if (sourceSummary)
      setSources((previous) => keepSame(previous, sourceSummary.groups));
    const sessionSummary = value(session);
    if (sessionSummary)
      setSessions((previous) => keepSame(previous, sessionSummary.groups));
    const answered = [
      daySummary,
      modelSummary,
      sourceSummary,
      sessionSummary,
    ].filter((item): item is UsageSummary => item !== null);
    if (answered.length > 0)
      setTruncated(answered.some((item) => item.truncated));
    setReadError(
      failure && failure.status === 'rejected'
        ? usageErrorMessage(failure.reason)
        : null,
    );
    setLoaded(true);
  }, [agentId, allAgents, days, clock]);

  useEffect(() => {
    unmounted.current = false;
    return () => {
      unmounted.current = true;
    };
  }, []);

  useEffect(() => {
    if (!enabled || (!agentId && !allAgents)) return;
    // State changes only after all awaited daemon responses have settled.
    // eslint-disable-next-line react/set-state-in-effect
    void load();
  }, [enabled, agentId, allAgents, days, epoch, load]);

  const refresh = useCallback(() => {
    setExportError(null);
    void load();
  }, [load]);

  const exportCsv = useCallback(async () => {
    const owner = agentRef.current;
    if (!owner && !allAgents) return false;
    // The range as of the click, from the current selection and clock: the
    // last read may be older than either.
    const { from, to } = usageRange(daysRef.current, clock.current());
    setExportError(null);
    try {
      const text = await daemon.exportUsageCsv({
        from,
        to,
        ...(allAgents ? {} : { agentId: owner! }),
      });
      downloadText(usageCsvFilename({ from, to }), text, 'text/csv');
      return true;
    } catch (error) {
      // The daemon's own words for a refused range; anything else is generic.
      if (!unmounted.current)
        setExportError(
          error instanceof DaemonHttpError && error.status === 400
            ? error.message
            : USAGE_EXPORT_FAILED,
        );
      return false;
    }
  }, [clock, agentRef, daysRef, allAgents]);

  return {
    range,
    days: dayList,
    models,
    sources,
    sessions,
    totals,
    today: dayList.length > 0 ? dayList[dayList.length - 1].totals : null,
    truncated,
    loaded,
    error: exportError ?? readError?.message ?? null,
    errorStatus: exportError ? null : (readError?.status ?? null),
    refresh,
    exportCsv,
  };
}

import { describe, expect, it } from 'vitest';
import type { AgentMemory, Run, UsageSummary } from '@animaos/sdk';

import { buildWeeklyReport, formatMicros, reportWindow } from './weekly-report';

const now = new Date(2026, 9, 18, 15, 0);
const { from, to } = reportWindow(now);
const day = (offset: number) => from + offset * 86_400_000 + 3_600_000;

function run(overrides: Partial<Run>): Run {
  return {
    id: 'run',
    sessionId: 'chat:main',
    status: 'completed',
    createdAtMs: day(1),
    finishedAtMs: day(1),
    error: null,
    actionReceipts: [],
    ...overrides,
  } as Run;
}

const receipt = (name: string, at: number, status = 'succeeded') => ({
  toolCallId: `${name}-${at}`,
  name,
  status: status as 'succeeded',
  startedAtMs: at,
  finishedAtMs: at,
  durationMs: 1,
  argumentsPreview: '{}',
  argumentsTruncated: false,
  resultPreview: null,
  resultTruncated: false,
  durability: 'persistent' as const,
});

describe('weekly report', () => {
  it('covers midnight six days ago until now', () => {
    expect(new Date(from).getDate()).toBe(12);
    expect(new Date(from).getHours()).toBe(0);
    expect(to).toBe(now.getTime());
  });

  it('counts only this week, groups what changed, and lists failures newest first', () => {
    const report = buildWeeklyReport({
      runs: [
        run({
          id: 'a',
          actionReceipts: [
            receipt('edit_file', day(1)),
            receipt('edit_file', day(2)),
            receipt('bash', day(2), 'failed'),
            receipt('read_file', day(2)),
          ],
        }),
        run({ id: 'old', createdAtMs: from - 1, finishedAtMs: from - 1 }),
        run({
          id: 'f1',
          status: 'failed',
          finishedAtMs: day(3),
          error: { code: 'model', message: 'The model key was rejected.' },
        }),
        run({ id: 'f2', status: 'failed', finishedAtMs: day(5), error: null }),
      ],
      usage: {
        totals: { costMicros: 3_800_000, unpricedCalls: 2 },
        groups: [{ key: '2026-10-13', totals: { costMicros: 900_000 } }],
      } as UsageSummary,
      memories: [
        { id: 'm1', content: 'Prefers window seats', createdAt: day(4) },
        { id: 'm0', content: 'Old fact', createdAt: from - 10 },
      ] as AgentMemory[],
      from,
      to,
    });
    expect(report.finished).toBe(1);
    expect(report.actions).toBe(2);
    expect(report.highlights).toEqual([{ title: 'Changed a file', count: 2 }]);
    expect(report.failed.map((issue) => issue.reason)).toEqual([
      'It stopped with an error.',
      'The model key was rejected.',
    ]);
    expect(report.spentMicros).toBe(3_800_000);
    expect(report.unpricedCalls).toBe(2);
    expect(report.days).toHaveLength(7);
    expect(report.days[1].micros).toBe(900_000);
    expect(report.learned.map((memory) => memory.id)).toEqual(['m1']);
  });

  it('formats money without inventing precision', () => {
    expect(formatMicros(0)).toBe('$0.00');
    expect(formatMicros(3_800_000)).toBe('$3.80');
    expect(formatMicros(4_000)).toBe('< $0.01');
    expect(formatMicros(12_400_000)).toBe('$12');
  });
});

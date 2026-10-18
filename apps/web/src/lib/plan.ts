/** The tool the agent calls to show its plan before a bigger job; it waits
 *  for the owner like any approval. */
export const PLAN_TOOL = 'propose_plan';

export interface PlanStep {
  action: string;
  /** The step will ask the owner again before it acts. */
  asksFirst: boolean;
}

export interface Plan {
  title: string;
  steps: PlanStep[];
  estimate: string | null;
}

/** The plan in a `propose_plan` call's arguments, or null when they are
 *  cut short or not a plan. The model wrote them: callers show text only. */
export function parsePlan(argumentsJson: string): Plan | null {
  let value: unknown;
  try {
    value = JSON.parse(argumentsJson);
  } catch {
    return null;
  }
  if (typeof value !== 'object' || value === null) return null;
  const record = value as Record<string, unknown>;
  const title = typeof record.title === 'string' ? record.title.trim() : '';
  if (!title || !Array.isArray(record.steps)) return null;
  const steps: PlanStep[] = [];
  for (const step of record.steps) {
    if (typeof step === 'string' && step.trim()) {
      steps.push({ action: step.trim(), asksFirst: false });
    } else if (typeof step === 'object' && step !== null) {
      const entry = step as Record<string, unknown>;
      if (typeof entry.action === 'string' && entry.action.trim())
        steps.push({
          action: entry.action.trim(),
          asksFirst: entry.asks_first === true,
        });
    }
  }
  if (steps.length === 0) return null;
  const estimate =
    typeof record.estimate === 'string' && record.estimate.trim()
      ? record.estimate.trim()
      : null;
  return { title, steps, estimate };
}

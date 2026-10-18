import type { AgentDetail } from './types';
import { selectMainAgent } from './agent-access';

const STORAGE_KEY = 'anima.activeAgent';

export function readActiveAgent(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

export function saveActiveAgent(agentId: string): void {
  try {
    localStorage.setItem(STORAGE_KEY, agentId);
  } catch {
    /* Storage can be unavailable in private windows; switching still works. */
  }
}

/** The agents the owner talks with; helpers work behind them. */
export function companionAgents(
  agents: readonly AgentDetail[],
): AgentDetail[] {
  return agents.filter((agent) => agent.workspaceRole !== 'helper');
}

/** The companion the owner picked, or the default one when none is picked
 *  or the picked one is gone. */
export function selectActiveAgent(
  agents: readonly AgentDetail[],
  preferredId: string | null,
): AgentDetail | null {
  return (
    companionAgents(agents).find((agent) => agent.id === preferredId) ??
    selectMainAgent(agents)
  );
}

import { useCallback, useSyncExternalStore } from 'react';

export const ORB_COLORS = [
  { id: 'pink', name: 'Anima pink', hex: '#E94DAB' },
  { id: 'blue', name: 'Blue', hex: '#66B7FF' },
  { id: 'amber', name: 'Amber', hex: '#F2B44B' },
  { id: 'green', name: 'Green', hex: '#72D59A' },
  { id: 'coral', name: 'Coral', hex: '#FF766E' },
  { id: 'violet', name: 'Violet', hex: '#A78BFA' },
  { id: 'cyan', name: 'Cyan', hex: '#50D5D4' },
  { id: 'paper', name: 'Paper', hex: '#ECE6DA' },
] as const;

export type OrbMotion = 'calm' | 'lively' | 'restless';
export type OrbGrain = 'smooth' | 'fine' | 'chunky';
export interface AgentOrbStyle {
  color: string;
  motion: OrbMotion;
  grain: OrbGrain;
}

export const DEFAULT_AGENT_ORB_STYLE: AgentOrbStyle = {
  color: '#E94DAB',
  motion: 'lively',
  grain: 'fine',
};
const DEFAULT_ORB_STYLE_SNAPSHOT = JSON.stringify(DEFAULT_AGENT_ORB_STYLE);

const KEY = 'anima.agent-orb-styles';
const CHANGE = 'anima:agent-orb-style';

function readAll(): Record<string, Partial<AgentOrbStyle>> {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(KEY) ?? '{}');
    return typeof value === 'object' && value !== null
      ? (value as Record<string, Partial<AgentOrbStyle>>)
      : {};
  } catch {
    return {};
  }
}

export function readAgentOrbStyle(agentId: string): AgentOrbStyle {
  const stored = readAll()[agentId];
  return {
    color:
      ORB_COLORS.find((item) => item.hex === stored?.color)?.hex ??
      DEFAULT_AGENT_ORB_STYLE.color,
    motion:
      stored?.motion === 'calm' ||
      stored?.motion === 'lively' ||
      stored?.motion === 'restless'
        ? stored.motion
        : DEFAULT_AGENT_ORB_STYLE.motion,
    grain:
      stored?.grain === 'smooth' ||
      stored?.grain === 'fine' ||
      stored?.grain === 'chunky'
        ? stored.grain
        : DEFAULT_AGENT_ORB_STYLE.grain,
  };
}

export function assignedOrbColors(): Record<string, string> {
  return Object.fromEntries(
    Object.entries(readAll())
      .filter(([, style]) => ORB_COLORS.some((color) => color.hex === style.color))
      .map(([agentId, style]) => [agentId, style.color!]),
  );
}

export function saveAgentOrbStyle(
  agentId: string,
  style: AgentOrbStyle,
): void {
  const all = readAll();
  all[agentId] = style;
  try {
    localStorage.setItem(KEY, JSON.stringify(all));
  } catch {
    // The preview remains usable for this render when storage is unavailable.
  }
  window.dispatchEvent(new CustomEvent(CHANGE, { detail: agentId }));
}

export function orbPixel(grain: OrbGrain): number {
  return grain === 'smooth' ? 1 : grain === 'chunky' ? 4 : 1.5;
}

export function orbEnergy(motion: OrbMotion): number {
  return motion === 'calm' ? 0.25 : motion === 'restless' ? 0.95 : 0.65;
}

export function useAgentOrbStyle(agentId: string): AgentOrbStyle {
  const subscribe = useCallback(
    (onChange: () => void) => {
      const update = (event: Event) => {
        if (event.type === 'storage') {
          if ((event as StorageEvent).key === KEY) onChange();
          return;
        }
        const changedId = (event as CustomEvent<string>).detail;
        if (!changedId || changedId === agentId) onChange();
      };
      window.addEventListener(CHANGE, update);
      window.addEventListener('storage', update);
      return () => {
        window.removeEventListener(CHANGE, update);
        window.removeEventListener('storage', update);
      };
    },
    [agentId],
  );
  const getSnapshot = useCallback(
    () => JSON.stringify(readAgentOrbStyle(agentId)),
    [agentId],
  );
  const snapshot = useSyncExternalStore(
    subscribe,
    getSnapshot,
    () => DEFAULT_ORB_STYLE_SNAPSHOT,
  );
  return JSON.parse(snapshot) as AgentOrbStyle;
}

import type { PresenceMode } from '../../lib/presence';
import { APPEARANCE_ACCENT_HEX } from '../../lib/appearance-preferences';

const LOOK: Record<PresenceMode, { accent: string; energy: number }> = {
  calm: { accent: '#E94DAB', energy: 0.3 },
  thinking: { accent: '#E94DAB', energy: 0.75 },
  speaking: { accent: '#E94DAB', energy: 0.6 },
  attention: { accent: '#F2B44B', energy: 0.55 },
};

/** How the agent's orb looks while it is in each state. */
export function presenceLook(
  mode: PresenceMode,
  /** The accent in effect, as `#rrggbb`. */
  accent: string = APPEARANCE_ACCENT_HEX.pink,
) {
  return { ...LOOK[mode], accent };
}

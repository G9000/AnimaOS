import type { PresenceMode } from '../../lib/presence';
import type { ChatBackground } from '../../lib/chat-background';
import {
  APPEARANCE_ACCENT_HEX,
  type AppearanceAccent,
} from '../../lib/appearance-preferences';
import { HaloOrb } from './HaloOrb';
import {
  orbPixel,
  useAgentOrbStyle,
} from '../../lib/orb-style-preferences';

const LOOK: Record<PresenceMode, { accent: string; energy: number }> = {
  calm: { accent: '#E94DAB', energy: 0.3 },
  thinking: { accent: '#E94DAB', energy: 0.75 },
  speaking: { accent: '#E94DAB', energy: 0.6 },
  attention: { accent: '#F2B44B', energy: 0.55 },
};

/** How the orb looks while the agent is in each state: the same orb in the
 *  background and in the header avatar. */
export function presenceLook(
  mode: PresenceMode,
  accent: AppearanceAccent = 'pink',
) {
  return { ...LOOK[mode], accent: APPEARANCE_ACCENT_HEX[accent] };
}

/** At rest in the quiet background the orb barely moves. */
const QUIET_CALM_ENERGY = 0.15;

/**
 * The console's background: a dark field with anima's orb. `full` keeps the
 * orb in the middle behind the conversation; `quiet` rests it at the edge so
 * the text stays clear, with a soft wash of the accent colour at the top; `off`
 * is the plain field.
 */
export function HaloBackdrop({
  mode,
  variant = 'full',
  accent = 'pink',
  agentId = '',
}: {
  mode: PresenceMode;
  variant?: ChatBackground;
  accent?: AppearanceAccent;
  agentId?: string;
}) {
  const orbStyle = useAgentOrbStyle(agentId);
  const look = presenceLook(mode, accent);
  const presenceEnergy =
    variant === 'quiet' && mode === 'calm' ? QUIET_CALM_ENERGY : look.energy;
  const energy = Math.min(
    presenceEnergy,
    orbStyle.motion === 'calm'
      ? 0.35
      : orbStyle.motion === 'restless'
        ? 1
        : 0.8,
  );
  return (
    <div
      className="halo-backdrop"
      data-mode={mode}
      data-variant={variant}
      aria-hidden
    >
      {variant !== 'off' && (
        <>
          <div
            className="halo-glow"
            style={{ ['--halo-accent' as string]: orbStyle.color }}
          />
          {variant === 'quiet' && <div className="halo-wash" />}
          <HaloOrb
            accent={orbStyle.color}
            energy={energy}
            pixel={orbPixel(orbStyle.grain)}
            className="halo-backdrop-orb"
          />
        </>
      )}
    </div>
  );
}

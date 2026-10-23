import { revealInvisible } from '../../lib/skills';
import type { PresenceMode } from '../../lib/presence';
import type { ChatBackground } from '../../lib/chat-background';
import { HaloOrb } from './HaloOrb';

const LOOK: Record<PresenceMode, { accent: string; energy: number }> = {
  calm: { accent: '#E94DAB', energy: 0.3 },
  thinking: { accent: '#E94DAB', energy: 0.75 },
  speaking: { accent: '#E94DAB', energy: 0.6 },
  listening: { accent: '#E94DAB', energy: 0.95 },
  attention: { accent: '#F2B44B', energy: 0.55 },
};

/** How the orb looks while the agent is in each state: the same orb in the
 *  background and in the header avatar. */
export function presenceLook(mode: PresenceMode) {
  return LOOK[mode];
}

/** At rest in the quiet background the orb barely moves. */
const QUIET_CALM_ENERGY = 0.15;

/**
 * The console's background: a dark field with anima's orb. `full` keeps the
 * orb in the middle behind the conversation; `quiet` rests it at the edge so
 * the text stays clear, with a soft wash of the accent colour at the top; `off`
 * is the plain field. While the owner talks the orb comes forward and shows
 * their words, whichever background is chosen.
 */
export function HaloBackdrop({
  mode,
  words = '',
  variant = 'full',
}: {
  mode: PresenceMode;
  words?: string;
  variant?: ChatBackground;
}) {
  const look = LOOK[mode];
  const energy =
    variant === 'quiet' && mode === 'calm' ? QUIET_CALM_ENERGY : look.energy;
  const shown = revealInvisible(words.trim()).text;
  return (
    <>
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
              style={{ ['--halo-accent' as string]: look.accent }}
            />
            {variant === 'quiet' && <div className="halo-wash" />}
            <HaloOrb
              accent={look.accent}
              energy={energy}
              className="halo-backdrop-orb"
            />
          </>
        )}
      </div>
      {/* Above the chat, below the composer: the owner's words while they talk. */}
      {mode === 'listening' && (
        <p className="halo-words" aria-hidden>
          {shown || 'Listening…'}
        </p>
      )}
    </>
  );
}

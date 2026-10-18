import { revealInvisible } from '../../lib/skills';
import type { PresenceMode } from '../../lib/presence';
import { HaloOrb } from './HaloOrb';

const LOOK: Record<PresenceMode, { accent: string; energy: number }> = {
  calm: { accent: '#E94DAB', energy: 0.3 },
  thinking: { accent: '#E94DAB', energy: 0.75 },
  speaking: { accent: '#E94DAB', energy: 0.6 },
  listening: { accent: '#E94DAB', energy: 0.95 },
  attention: { accent: '#F2B44B', energy: 0.55 },
};

/**
 * The console's background: a dark field with anima's orb in the middle.
 * While the owner talks the orb comes forward and shows their words; the
 * rest of the time it stays behind the conversation.
 */
export function HaloBackdrop({
  mode,
  words = '',
}: {
  mode: PresenceMode;
  words?: string;
}) {
  const look = LOOK[mode];
  const shown = revealInvisible(words.trim()).text;
  return (
    <>
      <div className="halo-backdrop" data-mode={mode} aria-hidden>
        <div
          className="halo-glow"
          style={{ ['--halo-accent' as string]: look.accent }}
        />
        <HaloOrb
          accent={look.accent}
          energy={look.energy}
          className="halo-backdrop-orb"
        />
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

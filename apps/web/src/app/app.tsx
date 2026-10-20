import { ViewHarness } from '../ViewHarness';
import { ConsoleSessionNotice } from '../components/ConsoleSessionNotice';
import type { ConsoleSessionClaim } from '../lib/console-session';

export function App({
  consoleClaim,
}: {
  /** Trading the launch code this console was opened with, if any. */
  consoleClaim?: Promise<ConsoleSessionClaim>;
} = {}) {
  return (
    <div className="app-viewport safe-app-frame relative flex flex-col overflow-hidden bg-abyss font-sans text-ink antialiased">
      {/* neutral spatial field */}
      <div className="ambient" aria-hidden />

      {consoleClaim && <ConsoleSessionNotice claim={consoleClaim} />}
      <ViewHarness />
    </div>
  );
}

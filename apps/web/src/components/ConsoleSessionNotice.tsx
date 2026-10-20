import { useEffect, useState } from 'react';

import type { ConsoleSessionClaim } from '../lib/console-session';
import { ErrorBanner } from './ui-bits';

/** Tells the owner when the launch link that opened this console did not
 *  open a session, above all when someone else had already used it. */
export function ConsoleSessionNotice({
  claim,
}: {
  claim: Promise<ConsoleSessionClaim>;
}) {
  const [warning, setWarning] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    void claim.then((outcome) => {
      if (current && outcome.kind === 'refused') setWarning(outcome.warning);
    });
    return () => {
      current = false;
    };
  }, [claim]);

  if (!warning) return null;
  return (
    <div
      role="alert"
      className="absolute inset-x-0 top-3 z-50 mx-auto w-full max-w-xl px-4"
    >
      <ErrorBanner message={warning} onDismiss={() => setWarning(null)} />
    </div>
  );
}

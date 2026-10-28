import { createContext, useContext } from 'react';

export type OAuthProvider = 'google' | 'microsoft';

/** Opens the page's OAuth app setup at one provider's fields. Null outside a
 *  connections page. */
export const OAuthSetupContext = createContext<
  ((provider: OAuthProvider) => void) | null
>(null);

/**
 * What a service says when its provider has no app credentials yet, with a
 * button that takes the owner to them: it closes the service's drawer, opens
 * OAuth app setup and puts the cursor in that provider's first field.
 */
export function OAuthSetupLink({
  provider,
  lead,
}: {
  provider: OAuthProvider;
  /** Replaces the default sentence when a service needs more said. */
  lead?: string;
}) {
  const open = useContext(OAuthSetupContext);
  const name = provider === 'google' ? 'Google' : 'Microsoft';
  return (
    <div className="connections-setup-hint">
      <p>{lead ?? `Connecting needs your ${name} app credentials first.`}</p>
      {open && (
        <button
          type="button"
          onClick={(event) => {
            const drawer = event.currentTarget.closest('dialog');
            if (drawer && typeof drawer.close === 'function') drawer.close();
            open(provider);
          }}
        >
          Add {name} credentials
        </button>
      )}
    </div>
  );
}

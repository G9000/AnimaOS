import { useResetOnChange } from '../hooks/useResetOnChange';
import { useState } from 'react';
import { useDriveIntegrations } from '../hooks/useDriveIntegrations';
import { parseDriveSelection } from '../lib/drive-selection';
import { ServiceRow } from './ServiceRow';
import { BrandIcon } from './BrandIcon';
import { OAuthSetupLink } from './OAuthSetupLink';

const button =
  'rounded-lg border border-line px-3 py-2 text-xs text-ink-2 disabled:opacity-50';
const STATUS = {
  active: 'Connected',
  pairing: 'Waiting for Google consent',
  reauthRequired: 'Fresh Google consent required',
  disconnected: 'Disconnected locally',
};

export function GoogleDriveCard({
  agentId,
  online = true,
  epoch = 0,
  disabled: parentDisabled = false,
}: {
  agentId: string;
  online?: boolean;
  epoch?: number;
  disabled?: boolean;
}) {
  const integration = useDriveIntegrations(
    agentId,
    online,
    epoch,
    parentDisabled,
  );
  const [selection, setSelection] = useState('');
  const [confirmDisconnect, setConfirmDisconnect] = useState(false);
  useResetOnChange([agentId], () => {
    setSelection('');
    setConfirmDisconnect(false);
  });
  useResetOnChange([online, epoch], () => {
    setConfirmDisconnect(false);
  });
  const connector = integration.status?.connector ?? null;
  useResetOnChange([connector?.id, connector?.status], () => {
    setConfirmDisconnect(false);
  });
  const selected = parseDriveSelection(selection);
  const fixedSelection =
    connector !== null && connector.status !== 'disconnected';
  const disabled =
    parentDisabled ||
    !online ||
    integration.busy ||
    integration.loading ||
    !integration.available;
  const configured = integration.status?.configured === true;
  return (
    <ServiceRow
      id="drive"
      group="google"
      title="Google Drive"
      summary="Read files you choose"
      icon={<BrandIcon brand="google-drive" />}
      status={
        connector
          ? integration.available
            ? STATUS[connector.status]
            : 'Status unavailable'
          : integration.status
            ? configured
              ? 'Not connected'
              : 'Setup required'
            : ''
      }
    >
      <p className="text-xs leading-relaxed text-ink-2">
        Google gives this app read access to your whole Drive, but Anima only
        opens the files you list below. Docs and text are read; other formats
        return details only. It can’t edit, delete or share anything.
      </p>
      {!configured && (
        <OAuthSetupLink
          provider="google"
          lead="Connecting needs your Google app credentials first, with the Drive API turned on in your Google Cloud project."
        />
      )}
      {connector && (
        <div className="space-y-2 rounded-xl border border-line p-3">
          <p className="text-sm text-ink">
            {connector.accountLabel ?? 'Google account'}
          </p>
          <p className="text-xs text-ink-3">
            {integration.available
              ? STATUS[connector.status]
              : 'Status unavailable — refresh to verify'}
          </p>
          <p className="text-xs text-ink-3">
            {connector.selectedFileIds.length} selected files
          </p>
          <ul
            aria-label="Connected selected file IDs"
            className="space-y-1 break-all font-mono text-xs text-ink-2"
          >
            {connector.selectedFileIds.map((id) => (
              <li key={id}>{id}</li>
            ))}
          </ul>
          {fixedSelection && (
            <p className="text-xs text-ink-3">
              Disconnect and reconnect to change selected files. Fresh consent
              keeps this exact selection and account.
            </p>
          )}
          {connector.cleanupPending && (
            <p role="status" className="text-xs text-ink-2">
              Credential cleanup is pending.{' '}
              {connector.status === 'disconnected'
                ? 'This agent’s Drive authority is revoked locally.'
                : 'Previous credentials are queued for local cleanup.'}{' '}
              Refresh status to check cleanup. Google account permission may
              still exist.
            </p>
          )}
          {connector.status === 'pairing' && (
            <p className="text-xs text-ink-3">
              Finish Google consent, then refresh status. If the consent window
              did not open, use the link here. A saved pending connection after
              restart needs a fresh consent attempt.
            </p>
          )}
          {(connector.status === 'pairing' ||
            connector.status === 'reauthRequired') && (
            <button
              type="button"
              className={button}
              disabled={disabled || !configured}
              onClick={() =>
                void integration.connect({
                  selectedFileIds: [...connector.selectedFileIds],
                })
              }
            >
              {connector.status === 'pairing'
                ? 'Restart Google consent'
                : 'Reconnect selected files'}
            </button>
          )}
          {connector.status !== 'disconnected' && (
            <div className="space-y-2">
              {confirmDisconnect ? (
                <>
                  <p className="text-xs text-ink-2">
                    Only this agent’s Drive access is removed locally. Gmail and
                    Calendar connections are preserved. This does not revoke
                    Google’s account-wide app permission.
                  </p>
                  <button
                    type="button"
                    className={button}
                    disabled={disabled}
                    onClick={() => {
                      setConfirmDisconnect(false);
                      void integration.disconnect(connector.id);
                    }}
                  >
                    Confirm local disconnect
                  </button>
                  <button
                    type="button"
                    className={button}
                    onClick={() => setConfirmDisconnect(false)}
                  >
                    Cancel disconnect
                  </button>
                </>
              ) : (
                <button
                  type="button"
                  className={button}
                  disabled={disabled}
                  onClick={() => setConfirmDisconnect(true)}
                >
                  Disconnect Drive
                </button>
              )}
            </div>
          )}
        </div>
      )}
      {!fixedSelection && (
        <div className="space-y-2">
          <label className="block text-xs text-ink-2">
            Selected Drive files
            <textarea
              className="field mt-1 min-h-24 w-full resize-y"
              disabled={integration.busy}
              value={selection}
              onChange={(event) => setSelection(event.target.value)}
              placeholder="File ID or Google Drive/Docs file link, one per line"
              maxLength={10000}
            />
          </label>
          <p className="text-xs text-ink-3">
            Choose 1–16 unique files. Links are parsed locally into IDs; parsing
            does not fetch files or grant access. Folder links are unavailable.
          </p>
          {selected.ids ? (
            <>
              <p className="text-xs text-ink-2">
                Review these exact IDs before opening consent:
              </p>
              <ul
                aria-label="Selected file IDs preview"
                className="space-y-1 break-all font-mono text-xs text-ink-2"
              >
                {selected.ids.map((id) => (
                  <li key={id}>{id}</li>
                ))}
              </ul>
            </>
          ) : (
            selection.trim() && (
              <p role="alert" className="text-xs text-danger">
                {selected.error}
              </p>
            )
          )}
          <button
            type="button"
            className={button}
            disabled={disabled || !configured || !selected.ids}
            onClick={() => {
              if (selected.ids)
                void integration.connect({
                  selectedFileIds: [...selected.ids],
                });
            }}
          >
            Connect selected files
          </button>
        </div>
      )}
      {integration.consentUrl && (
        <a
          className="inline-block rounded-lg border border-line px-3 py-2 text-xs text-accent"
          href={integration.consentUrl}
          target="_blank"
          rel="noopener noreferrer"
        >
          Finish Google consent
        </a>
      )}
      <div>
        <button
          type="button"
          className={button}
          disabled={!online || integration.busy || integration.loading}
          onClick={() => void integration.refresh()}
        >
          {integration.loading ? 'Loading Drive…' : 'Refresh Drive status'}
        </button>
      </div>
      {integration.error && (
        <p role="alert" className="text-xs text-danger">
          {integration.error}
        </p>
      )}
      <p className="text-xs leading-relaxed text-ink-3">
        To remove Google’s app permission for the whole account, review{' '}
        <a
          href="https://myaccount.google.com/connections"
          target="_blank"
          rel="noopener noreferrer"
          className="underline"
        >
          Google account app access
        </a>
        . Removing that permission may also disconnect Gmail and Calendar using
        the same Google app.
      </p>
    </ServiceRow>
  );
}

import { useState } from 'react';
import {
  showSystemNotification,
  useNotificationPermission,
} from '../lib/system-notifications';
import { BellIcon } from './icons';

const STATUS: Record<
  ReturnType<typeof useNotificationPermission>['permission'],
  string
> = {
  granted: 'On for this browser',
  default: 'Off. Your browser will ask once.',
  denied:
    'Blocked by your browser. Allow notifications for this site in its settings, then come back.',
  unsupported: 'This browser cannot show notifications.',
};

/** Desktop notifications for this browser: what anima tells you about while
 *  you are in another tab or app, and the permission it needs. */
export function NotificationsPage() {
  const { permission, request } = useNotificationPermission();
  const [tested, setTested] = useState<boolean | null>(null);
  return (
    <div className="settings-form notifications-settings">
      <section className="settings-group" aria-labelledby="notifications-desktop">
        <h2 id="notifications-desktop" className="settings-group-title">
          Desktop notifications
        </h2>
        <div className="settings-list">
          <div className="settings-item">
            <span className="settings-item-icon" aria-hidden="true">
              <BellIcon size={16} />
            </span>
            <div className="settings-item-text">
              <strong>Notify me when I am elsewhere</strong>
              <small role="status">{STATUS[permission]}</small>
            </div>
            {permission === 'default' && (
              <button
                type="button"
                className="settings-item-action"
                onClick={() => void request()}
              >
                Turn on
              </button>
            )}
            {permission === 'granted' && (
              <button
                type="button"
                className="settings-item-action"
                onClick={() =>
                  setTested(
                    showSystemNotification('Notifications are on', {
                      body: 'This is how anima reaches you while you are in another tab or app.',
                      tag: 'notifications-test',
                    }),
                  )
                }
              >
                Send a test
              </button>
            )}
          </div>
        </div>
        {tested === false && (
          <p role="alert" className="settings-group-help">
            The test did not show. Check that notifications are allowed for
            your browser in your system settings.
          </p>
        )}
      </section>
      <section className="settings-group" aria-labelledby="notifications-what">
        <h2 id="notifications-what" className="settings-group-title">
          What you hear about
        </h2>
        <div className="settings-list">
          <div className="settings-item">
            <div className="settings-item-text">
              <strong>Approvals</strong>
              <small>When an agent waits for you to allow or deny something.</small>
            </div>
          </div>
          <div className="settings-item">
            <div className="settings-item-text">
              <strong>Automations and reminders</strong>
              <small>What scheduled work says in the main chat.</small>
            </div>
          </div>
          <div className="settings-item">
            <div className="settings-item-text">
              <strong>With anima closed</strong>
              <small>
                Not available yet. Notifications need anima open in a tab;
                Telegram or Discord reach you when it is closed.
              </small>
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}

import { useCallback, useEffect, useState } from 'react';

/** Where desktop notifications stand in this browser. */
export type NotificationPermissionState =
  | 'unsupported'
  | 'default'
  | 'granted'
  | 'denied';

function currentPermission(): NotificationPermissionState {
  if (typeof Notification === 'undefined') return 'unsupported';
  return Notification.permission;
}

/**
 * The browser's notification permission, kept current: the owner can change
 * it from the site settings while the page is open, and the Permissions API
 * says so where it exists. Asking is always the owner's click.
 */
export function useNotificationPermission() {
  const [permission, setPermission] =
    useState<NotificationPermissionState>(currentPermission);
  useEffect(() => {
    let status: PermissionStatus | null = null;
    let cancelled = false;
    const update = () => setPermission(currentPermission());
    navigator.permissions
      ?.query({ name: 'notifications' })
      .then((result) => {
        if (cancelled) return;
        status = result;
        result.addEventListener('change', update);
      })
      .catch(() => undefined);
    window.addEventListener('focus', update);
    return () => {
      cancelled = true;
      status?.removeEventListener('change', update);
      window.removeEventListener('focus', update);
    };
  }, []);
  const request = useCallback(async () => {
    if (typeof Notification === 'undefined') return;
    try {
      setPermission(await Notification.requestPermission());
    } catch {
      setPermission(currentPermission());
    }
  }, []);
  return { permission, request };
}

/** Shows a desktop notification; a click brings this tab forward. Returns
 *  whether one was shown. */
export function showSystemNotification(
  title: string,
  options: { body: string; tag: string },
): boolean {
  if (currentPermission() !== 'granted') return false;
  try {
    const notification = new Notification(title, options);
    notification.onclick = () => {
      window.focus();
      notification.close();
    };
    return true;
  } catch {
    return false;
  }
}

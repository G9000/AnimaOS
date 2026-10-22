import type { AutomationMessageMeta } from '@animaos/sdk';


function clock(ms: number): string {
  return new Date(ms).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
  });
}

/** Top-right, under the bar: newest on top, three at most. */
export const MAX_TOASTS = 3;
/** A report or note leaves by itself after this long. */
export const TOAST_AUTO_DISMISS_MS = 8_000;

export type ToastKind = 'reminder' | 'report' | 'note';

export interface AutomationToast {
  /** The main-chat message it announces. */
  id: string;
  kind: ToastKind;
  meta: AutomationMessageMeta;
}

export function toastKind(meta: AutomationMessageMeta): ToastKind {
  if (meta.kind === 'automationNote') return 'note';
  return meta.reminder ? 'reminder' : 'report';
}

/** A reminder stays until the owner acts; the rest leave on their own. */
export function autoDismisses(toast: Pick<AutomationToast, 'kind'>): boolean {
  return toast.kind !== 'reminder';
}

export function makeToast(
  messageId: string,
  meta: AutomationMessageMeta,
): AutomationToast {
  return { id: messageId, kind: toastKind(meta), meta };
}

/**
 * Adds a toast on top. An announcement already shown is not shown twice.
 * Past `MAX_TOASTS` the oldest quiet toast goes first, so a reminder that
 * waits for the owner is the last to be pushed out.
 */
export function pushToast(
  toasts: readonly AutomationToast[],
  toast: AutomationToast,
): AutomationToast[] {
  if (toasts.some((item) => item.id === toast.id)) return [...toasts];
  const next = [toast, ...toasts];
  while (next.length > MAX_TOASTS) {
    let drop = -1;
    for (let index = next.length - 1; index > 0; index -= 1)
      if (autoDismisses(next[index])) {
        drop = index;
        break;
      }
    next.splice(drop < 0 ? next.length - 1 : drop, 1);
  }
  return next;
}

export function dismissToast(
  toasts: readonly AutomationToast[],
  id: string,
): AutomationToast[] {
  return toasts.filter((toast) => toast.id !== id);
}

/** What the owner is looking at when a message arrives. */
export interface ChatAttention {
  /** The main chat is the open conversation (not a page, not a side thread). */
  viewingMainChat: boolean;
  /** Its transcript is scrolled to the newest message. */
  atBottom: boolean;
  /** The browser tab is in the background. */
  documentHidden: boolean;
}

/** A message the owner is already looking at needs no toast. */
export function shouldToast(attention: ChatAttention): boolean {
  return (
    attention.documentHidden ||
    !attention.viewingMainChat ||
    !attention.atBottom
  );
}

export interface ToastText {
  /** The small line above a reminder ("Reminder · 2:00 PM"). */
  label: string | null;
  title: string;
  detail: string;
  /** The button that goes to the message. */
  action: 'Main chat' | 'Show' | 'Open';
}

/** The words of a toast and of its system notification. */
export function toastText(toast: AutomationToast): ToastText {
  const { meta } = toast;
  const time = clock(meta.firedAtMs);
  if (meta.kind === 'automationNote')
    return {
      label: null,
      title: `${meta.automationName} sent to Discord`,
      detail: `${meta.destinationLabel} · ${time}`,
      // Without a thread to open, the note in the main chat is all there is.
      action: meta.destinationSessionId ? 'Open' : 'Show',
    };
  if (meta.reminder)
    return {
      label: `Reminder · ${time}`,
      title: meta.automationName,
      detail: meta.body,
      action: 'Main chat',
    };
  return {
    label: null,
    title: `${meta.automationName} posted`,
    detail: `In main chat · ${time}`,
    action: 'Show',
  };
}

interface NotificationEnvironment {
  hidden: boolean;
  permission: string | undefined;
  create: (title: string, options: { body: string; tag: string }) => void;
}

function browserEnvironment(): NotificationEnvironment {
  return {
    hidden: typeof document !== 'undefined' && document.hidden,
    permission:
      typeof Notification === 'undefined' ? undefined : Notification.permission,
    create: (title, options) => {
      const notification = new Notification(title, options);
      notification.onclick = () => {
        window.focus();
        notification.close();
      };
    },
  };
}

/**
 * A system notification for a toast, only when the tab is in the background
 * and the owner already granted permission. Permission is never requested
 * here. Returns whether one was shown.
 */
export function notifyToast(
  toast: AutomationToast,
  environment: NotificationEnvironment = browserEnvironment(),
): boolean {
  if (!environment.hidden || environment.permission !== 'granted') return false;
  const text = toastText(toast);
  try {
    environment.create(text.title, { body: text.detail, tag: toast.id });
    return true;
  } catch {
    return false;
  }
}

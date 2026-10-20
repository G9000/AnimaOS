import { useEffect, useState } from 'react';
import {
  DaemonHttpError,
  type Automation,
} from '@animaOS-SWARM/sdk';

import { COMPANION_UNREACHABLE } from '../../lib/approvals';
import {
  SNOOZE_MINUTES,
  reminderState,
} from '../../lib/automation-cards';
import {
  TOAST_AUTO_DISMISS_MS,
  autoDismisses,
  toastText,
  type AutomationToast,
} from '../../lib/automation-toasts';
import { revealInvisible } from '../../lib/skills';
import {
  BellIcon,
  CheckIcon,
  HashIcon,
  SnoozeIcon,
  SparkIcon,
  TelegramIcon,
  XIcon,
} from '../icons';

export interface AutomationToastsProps {
  toasts: readonly AutomationToast[];
  automations: readonly Automation[];
  /** The automation list was read: a reminder that is no longer due leaves. */
  loaded: boolean;
  online: boolean;
  onDismiss: (id: string) => void;
  /** "Main chat", "Show" and "Open": goes to what the toast announced. */
  onOpen: (toast: AutomationToast) => void;
  onDone: (automation: Automation) => Promise<boolean>;
  onSnooze: (automation: Automation, minutes: number) => Promise<boolean>;
}

/** The stack top-right under the bar, where the Needs-you panel opens:
 *  newest on top, three at most. A reminder stays until the owner acts; a
 *  report or note leaves after a few seconds. */
export function AutomationToasts({
  toasts,
  automations,
  loaded,
  online,
  onDismiss,
  onOpen,
  onDone,
  onSnooze,
}: AutomationToastsProps) {
  const visible = toasts.filter((toast) => {
    if (toast.kind !== 'reminder' || !loaded) return true;
    const automation = automations.find(
      (item) => item.id === toast.meta.automationId,
    );
    return reminderState(automation, toast.meta) === 'due';
  });
  if (visible.length === 0) return null;
  return (
    <section className="automation-toasts" aria-label="Notifications">
      {visible.map((toast) =>
        toast.kind === 'reminder' ? (
          <ReminderToast
            key={toast.id}
            toast={toast}
            automation={automations.find(
              (item) => item.id === toast.meta.automationId,
            )}
            online={online}
            onDismiss={onDismiss}
            onOpen={onOpen}
            onDone={onDone}
            onSnooze={onSnooze}
          />
        ) : (
          <QuietToast
            key={toast.id}
            toast={toast}
            onDismiss={onDismiss}
            onOpen={onOpen}
          />
        ),
      )}
    </section>
  );
}

function ReminderToast({
  toast,
  automation,
  online,
  onDismiss,
  onOpen,
  onDone,
  onSnooze,
}: {
  toast: AutomationToast;
  automation: Automation | undefined;
  online: boolean;
  onDismiss: (id: string) => void;
  onOpen: (toast: AutomationToast) => void;
  onDone: AutomationToastsProps['onDone'];
  onSnooze: AutomationToastsProps['onSnooze'];
}) {
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const text = toastText(toast);
  const act = async (work: (target: Automation) => Promise<boolean>) => {
    if (!automation) return;
    setBusy(true);
    setProblem(null);
    try {
      await work(automation);
      onDismiss(toast.id);
    } catch (caught) {
      setProblem(
        caught instanceof DaemonHttpError
          ? caught.message
          : COMPANION_UNREACHABLE,
      );
      setBusy(false);
    }
  };
  const telegram =
    toast.meta.kind === 'automation' && toast.meta.delivery === 'telegram';
  return (
    <article
      className="automation-toast"
      data-kind="reminder"
      aria-label={`Reminder ${revealInvisible(text.title).text}`}
    >
      <header className="automation-toast-head">
        <span className="automation-card-needs">
          <BellIcon size={11} /> {text.label}
        </span>
        <button
          type="button"
          className="automation-toast-close"
          aria-label="Dismiss reminder"
          onClick={() => onDismiss(toast.id)}
        >
          <XIcon size={13} />
        </button>
      </header>
      <h3 className="automation-toast-title">
        {revealInvisible(text.title).text}
      </h3>
      <p className="automation-toast-detail">
        {revealInvisible(text.detail).text}
      </p>
      <div className="automation-toast-actions">
        <button
          type="button"
          className="automation-card-primary"
          disabled={busy || !online || !automation}
          onClick={() => void act(onDone)}
        >
          <CheckIcon size={13} /> Done
        </button>
        <button
          type="button"
          className="studio-tool-button"
          disabled={busy || !online || !automation}
          onClick={() =>
            void act((target) => onSnooze(target, SNOOZE_MINUTES))
          }
        >
          <SnoozeIcon size={13} /> Snooze {SNOOZE_MINUTES} min
        </button>
        <button
          type="button"
          className="automation-toast-open"
          onClick={() => onOpen(toast)}
        >
          {text.action} →
        </button>
      </div>
      {problem && (
        <p className="automation-problem" role="status">
          {problem}
        </p>
      )}
      <footer className="automation-toast-foot">
        {telegram ? (
          <span className="automation-card-also">
            <TelegramIcon size={12} /> Also sent to Telegram
          </span>
        ) : (
          <span />
        )}
        <span className="automation-toast-stays">Stays until you act</span>
      </footer>
    </article>
  );
}

function QuietToast({
  toast,
  onDismiss,
  onOpen,
}: {
  toast: AutomationToast;
  onDismiss: (id: string) => void;
  onOpen: (toast: AutomationToast) => void;
}) {
  const [paused, setPaused] = useState(false);
  const text = toastText(toast);
  const leaves = autoDismisses(toast);
  // Hovering or focusing a toast holds it; letting go starts the wait over.
  useEffect(() => {
    if (!leaves || paused) return;
    const timer = window.setTimeout(
      () => onDismiss(toast.id),
      TOAST_AUTO_DISMISS_MS,
    );
    return () => window.clearTimeout(timer);
  }, [leaves, paused, onDismiss, toast.id]);
  const Icon = toast.kind === 'note' ? HashIcon : SparkIcon;
  return (
    <article
      className="automation-toast"
      data-kind={toast.kind}
      aria-label={revealInvisible(text.title).text}
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      onFocus={() => setPaused(true)}
      onBlur={() => setPaused(false)}
    >
      <div className="automation-toast-row">
        <Icon size={16} className="automation-card-icon" />
        <div className="automation-toast-lines">
          <h3 className="automation-toast-title">
            {revealInvisible(text.title).text}
          </h3>
          <p className="automation-toast-detail">
            {revealInvisible(text.detail).text}
          </p>
        </div>
        <button
          type="button"
          className="automation-toast-open"
          onClick={() => onOpen(toast)}
        >
          {text.action} →
        </button>
        <button
          type="button"
          className="automation-toast-close"
          aria-label="Dismiss"
          onClick={() => onDismiss(toast.id)}
        >
          <XIcon size={13} />
        </button>
      </div>
      <span
        key={paused ? 'held' : 'running'}
        className="automation-toast-progress"
        data-paused={paused || undefined}
        aria-hidden
      />
    </article>
  );
}

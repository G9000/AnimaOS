import { useState } from 'react';
import {
  DaemonHttpError,
  type AutomationCardMeta,
  type AutomationMessageMeta,
  type AutomationNoteMeta,
} from '@animaOS-SWARM/sdk';

import { COMPANION_UNREACHABLE } from '../../lib/approvals';
import {
  LATER_CHOICES,
  SNOOZE_MINUTES,
  reminderState,
  type AutomationCardActions,
  type ReminderState,
} from '../../lib/automation-cards';
import { describeTrigger } from '../../lib/automations';
import { revealInvisible } from '../../lib/skills';
import {
  BellIcon,
  CheckIcon,
  ChevronIcon,
  ClockIcon,
  HashIcon,
  SnoozeIcon,
  SparkIcon,
  TelegramIcon,
} from '../icons';
import { MarkdownMessage } from '../MarkdownMessage';
import { formatTime } from '../ui-bits';

/** An automation run's words in the main chat (the daemon's message
 *  metadata): a reminder or report card, or a note that the text went to
 *  Discord. Names and bodies are the companion's text, so invisible
 *  characters are shown as markers. */
export function AutomationMessageCard({
  meta,
  actions,
}: {
  meta: AutomationMessageMeta;
  actions?: AutomationCardActions;
}) {
  if (meta.kind === 'automationNote')
    return <DiscordNote meta={meta} actions={actions} />;
  return meta.reminder ? (
    <ReminderCard meta={meta} actions={actions} />
  ) : (
    <ReportCard meta={meta} actions={actions} />
  );
}

function isoTime(ms: number): string {
  return Number.isFinite(ms) ? new Date(ms).toISOString() : '';
}

function AlsoTelegram() {
  return (
    <span className="automation-card-also">
      <TelegramIcon size={12} /> Also sent to Telegram
    </span>
  );
}

function problemText(caught: unknown): string {
  return caught instanceof DaemonHttpError
    ? caught.message
    : COMPANION_UNREACHABLE;
}

function ReportCard({
  meta,
  actions,
}: {
  meta: AutomationCardMeta;
  actions?: AutomationCardActions;
}) {
  const automation = actions?.automations.find(
    (item) => item.id === meta.automationId,
  );
  const name = revealInvisible(meta.automationName).text;
  const trigger = automation
    ? ` · ${revealInvisible(describeTrigger(automation.trigger)).text}`
    : '';
  return (
    <article
      className="automation-card"
      data-kind="report"
      aria-label={`Automation ${name}`}
    >
      <header className="automation-card-head">
        <SparkIcon size={16} className="automation-card-icon" />
        <div className="automation-card-titles">
          <h3 className="automation-card-title">{name}</h3>
          <p className="automation-card-sub">Automation{trigger}</p>
        </div>
        <time
          className="automation-card-time"
          dateTime={isoTime(meta.firedAtMs)}
        >
          {formatTime(meta.firedAtMs)}
        </time>
      </header>
      <div className="automation-card-body">
        <MarkdownMessage>{revealInvisible(meta.body).text}</MarkdownMessage>
      </div>
      {meta.delivery === 'telegram' && (
        <footer className="automation-card-foot">
          <AlsoTelegram />
        </footer>
      )}
    </article>
  );
}

function stateIcon(state: ReminderState | null) {
  switch (state) {
    case 'done':
    case 'gone':
      return CheckIcon;
    case 'snoozed':
      return SnoozeIcon;
    case 'earlier':
      return ClockIcon;
    default:
      return BellIcon;
  }
}

function ReminderCard({
  meta,
  actions,
}: {
  meta: AutomationCardMeta;
  actions?: AutomationCardActions;
}) {
  const [busy, setBusy] = useState(false);
  const [laterOpen, setLaterOpen] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const automation = actions?.automations.find(
    (item) => item.id === meta.automationId,
  );
  const state: ReminderState | null =
    actions?.loaded === true ? reminderState(automation, meta) : null;
  const name = revealInvisible(meta.automationName).text;
  const body = revealInvisible(meta.body).text;
  const act = async (work: () => Promise<boolean>) => {
    setBusy(true);
    setProblem(null);
    setLaterOpen(false);
    try {
      await work();
    } catch (caught) {
      setProblem(problemText(caught));
    }
    setBusy(false);
  };
  const snooze = (minutes: number) => {
    if (!actions || !automation) return;
    void act(() => actions.onSnooze(automation, minutes));
  };

  let status = `Reminder · ${formatTime(meta.firedAtMs)}`;
  if (state === 'snoozed' && automation)
    status = `Snoozed until ${formatTime(automation.nextDueAtMs)} · pops up again then`;
  else if (state === 'done')
    status = automation?.doneAtMs
      ? `Done at ${formatTime(automation.doneAtMs)}`
      : 'Done';
  else if (state === 'earlier')
    status = `Earlier reminder · ${formatTime(meta.firedAtMs)} · it came up again later`;
  else if (state === 'gone') status = 'Done · cleared from Automations';
  const due = state === 'due';
  const Icon = stateIcon(state);

  return (
    <article
      className="automation-card"
      data-kind="reminder"
      data-state={state ?? 'unknown'}
      aria-label={`Reminder ${name}`}
    >
      <header className="automation-card-head">
        <Icon size={16} className="automation-card-icon" />
        <div className="automation-card-titles">
          <h3 className="automation-card-title">{name}</h3>
          <p className="automation-card-sub">{status}</p>
        </div>
        {due && <span className="automation-card-needs">Needs you</span>}
      </header>
      {(due || state === null) && (
        <div className="automation-card-body">
          <MarkdownMessage>{body}</MarkdownMessage>
        </div>
      )}
      {due && actions && automation && (
        <footer className="automation-card-foot">
          <div className="automation-card-actions">
            <button
              type="button"
              className="automation-card-primary"
              disabled={busy || !actions.online}
              onClick={() => void act(() => actions.onDone(automation))}
            >
              <CheckIcon size={13} /> Done
            </button>
            <button
              type="button"
              className="studio-tool-button"
              disabled={busy || !actions.online}
              onClick={() => snooze(SNOOZE_MINUTES)}
            >
              <SnoozeIcon size={13} /> Snooze {SNOOZE_MINUTES} min
            </button>
            <button
              type="button"
              className="studio-tool-button"
              aria-expanded={laterOpen}
              disabled={busy || !actions.online}
              onClick={() => setLaterOpen((open) => !open)}
            >
              <ChevronIcon size={13} /> Later…
            </button>
            {laterOpen && (
              <span
                className="automation-card-later"
                role="group"
                aria-label="Snooze for"
              >
                {LATER_CHOICES.map((choice) => (
                  <button
                    key={choice.minutes}
                    type="button"
                    className="studio-tool-button"
                    disabled={busy || !actions.online}
                    onClick={() => snooze(choice.minutes)}
                  >
                    {choice.label}
                  </button>
                ))}
              </span>
            )}
          </div>
          {meta.delivery === 'telegram' && <AlsoTelegram />}
        </footer>
      )}
      {state === null && meta.delivery === 'telegram' && (
        <footer className="automation-card-foot">
          <AlsoTelegram />
        </footer>
      )}
      {problem && (
        <p className="automation-problem automation-card-problem" role="status">
          {problem}
        </p>
      )}
    </article>
  );
}

function DiscordNote({
  meta,
  actions,
}: {
  meta: AutomationNoteMeta;
  actions?: AutomationCardActions;
}) {
  const [open, setOpen] = useState(false);
  const name = revealInvisible(meta.automationName).text;
  const where = revealInvisible(meta.destinationLabel).text;
  const sessionId = meta.destinationSessionId;
  return (
    <div className="automation-note" data-open={open || undefined}>
      <div className="automation-note-row">
        <HashIcon size={14} className="automation-card-icon" />
        <span className="automation-note-text">
          {name} went to Discord · {where}
        </span>
        <time
          className="automation-card-time"
          dateTime={isoTime(meta.firedAtMs)}
        >
          {formatTime(meta.firedAtMs)}
        </time>
        {sessionId && actions && (
          <button
            type="button"
            className="automation-note-link"
            onClick={() =>
              actions.onOpenSession(sessionId, meta.destinationMessageId)
            }
          >
            Open thread
          </button>
        )}
        <button
          type="button"
          className="automation-note-toggle"
          aria-expanded={open}
          aria-label={`${open ? 'Hide' : 'Show'} what ${name} sent`}
          onClick={() => setOpen((value) => !value)}
        >
          <ChevronIcon size={14} />
        </button>
      </div>
      {open && (
        <div className="automation-note-body">
          <MarkdownMessage>{revealInvisible(meta.body).text}</MarkdownMessage>
        </div>
      )}
    </div>
  );
}

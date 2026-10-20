import {
  automationMessageMeta,
  type Automation,
  type AutomationCardMeta,
  type AutomationMessageLike,
} from '@animaOS-SWARM/sdk';

/** What a card in the chat reads and does: the companion's automations and
 *  the owner's two answers to a due reminder. */
export interface AutomationCardActions {
  automations: readonly Automation[];
  /** The list was read at least once: before that no state is known. */
  loaded: boolean;
  online: boolean;
  /** Resolve true when the daemon took it; reject with its message when it
   *  refused. */
  onDone: (automation: Automation) => Promise<boolean>;
  onSnooze: (automation: Automation, minutes: number) => Promise<boolean>;
  /** Opens a session, at one of its messages when given. */
  onOpenSession: (sessionId: string, messageId?: string) => void;
}

/** What a reminder card in the chat shows, derived from its automation:
 *  - `due`: it fired and waits for the owner (Done, Snooze, Later)
 *  - `snoozed`: re-armed, it pops up again at its next due time
 *  - `done`: the owner (or the companion) marked it done
 *  - `earlier`: it fired again since this card (a snooze ran out), so the
 *    newer card owns the actions
 *  - `gone`: the daemon removed it a day after it was done */
export type ReminderState = 'due' | 'snoozed' | 'done' | 'earlier' | 'gone';

/** A card more than this long before the automation's latest firing is an
 *  earlier one (the shortest snooze is a minute). */
const SUPERSEDED_AFTER_MS = 30_000;

/** The automation's state for a reminder card. `automation` is undefined once
 *  the record no longer exists. */
export function reminderState(
  automation: Automation | undefined,
  card: Pick<AutomationCardMeta, 'firedAtMs'>,
): ReminderState {
  if (!automation) return 'gone';
  if (automation.doneAtMs != null) return 'done';
  const once = automation.trigger.type === 'once';
  const fired = automation.lastFiredAtMs;
  if (!once || fired == null) return 'gone';
  if (fired - card.firedAtMs > SUPERSEDED_AFTER_MS) return 'earlier';
  if (!automation.enabled) return 'due';
  // Enabled and already fired: snoozed. A due time that has already passed
  // means it is about to pop up again, so it reads the same way.
  return 'snoozed';
}

/** The snooze choices (minutes): the first is the card's button, the rest
 *  sit behind "Later…". */
export const SNOOZE_MINUTES = 15;
export const LATER_CHOICES: readonly { minutes: number; label: string }[] = [
  { minutes: 60, label: '1 hour' },
  { minutes: 180, label: '3 hours' },
];

/** One-time automations that fired and were not marked done need the owner. */
export function reminderNeedsYou(automation: Automation): boolean {
  return (
    automation.trigger.type === 'once' &&
    automation.lastFiredAtMs != null &&
    automation.doneAtMs == null &&
    !automation.enabled
  );
}

/** Where an automation's output goes, for the Settings chips. */
export type DestinationChip =
  | { kind: 'main' }
  | { kind: 'telegram' }
  | { kind: 'discord'; label: string }
  | { kind: 'note' }
  | { kind: 'connector' };

/** `Nova · #home` to `#home`: the channel is the part owners recognise. */
export function discordChannelLabel(label: string): string {
  const parts = label.split(' · ');
  return parts[parts.length - 1] || label;
}

/**
 * Every run lands in the main chat; the destination adds a copy (Telegram) or
 * moves the text to Discord and leaves a note. A target the owner cannot
 * resolve (an unknown connector) is shown as a connected channel.
 */
export function destinationChips(
  automation: Pick<Automation, 'target'>,
  context: {
    telegramConnectorId: string | null;
    discordDestinations: readonly { connectorId: string; label: string }[];
  },
): DestinationChip[] {
  const { target } = automation;
  if (target.type === 'workspace') return [{ kind: 'main' }];
  if (target.connectorId === context.telegramConnectorId)
    return [{ kind: 'main' }, { kind: 'telegram' }];
  const discord = context.discordDestinations.find(
    (item) => item.connectorId === target.connectorId,
  );
  if (discord)
    return [
      { kind: 'discord', label: discordChannelLabel(discord.label) },
      { kind: 'note' },
    ];
  return [{ kind: 'main' }, { kind: 'connector' }];
}

export interface AutomationGroups {
  /** Interrupted runs that need a look before they run again. */
  needsReview: Automation[];
  /** One-time, on, not yet fired. */
  upcoming: Automation[];
  /** Everything that is not a one-time automation. */
  repeating: Automation[];
  /** One-time automations that fired (a due reminder, a snoozed one, a done
   *  one) until the daemon removes them a day after they are done. */
  done: Automation[];
}

export function isInterrupted(automation: Automation): boolean {
  return (
    automation.lastOutcome?.status === 'error' &&
    automation.lastOutcome.errorCode === 'schedule_run_interrupted'
  );
}

/** Sorts the Automations page: Needs review first, then upcoming (soonest
 *  first), repeating (oldest first, as listed), and done (latest first). */
export function groupAutomations(
  automations: readonly Automation[],
): AutomationGroups {
  const groups: AutomationGroups = {
    needsReview: [],
    upcoming: [],
    repeating: [],
    done: [],
  };
  for (const automation of automations) {
    if (isInterrupted(automation)) groups.needsReview.push(automation);
    else if (automation.trigger.type !== 'once')
      groups.repeating.push(automation);
    else if (automation.doneAtMs != null || automation.lastFiredAtMs != null)
      groups.done.push(automation);
    else groups.upcoming.push(automation);
  }
  groups.upcoming.sort((left, right) => left.nextDueAtMs - right.nextDueAtMs);
  groups.done.sort(
    (left, right) =>
      (right.doneAtMs ?? right.lastFiredAtMs ?? 0) -
      (left.doneAtMs ?? left.lastFiredAtMs ?? 0),
  );
  return groups;
}

/**
 * The newest loaded main-chat message an automation left: for one run when
 * `runId` is known, else for the automation. Null when it is not among the
 * loaded messages (the main chat then opens without an anchor).
 */
export function findAutomationMessage(
  messages: readonly (AutomationMessageLike & { id: string })[],
  target: { automationId: string; runId?: string | null },
): string | null {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const meta = automationMessageMeta(messages[index]);
    if (!meta) continue;
    if (target.runId ? meta.runId === target.runId : meta.automationId === target.automationId)
      return messages[index].id;
  }
  return null;
}

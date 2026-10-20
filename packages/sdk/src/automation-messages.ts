/**
 * What an automation run says in the companion's main chat. The daemon
 * appends an assistant message whose `text` is written for the model; the
 * owner reads the structured `metadata` instead. `automation` is a full card
 * (a reminder or a report); `automationNote` is a one-line note that the
 * text went somewhere else (a Discord channel).
 */

export interface AutomationCardMeta {
  kind: 'automation';
  automationId: string;
  automationName: string;
  /** A one-time automation: a reminder the owner can mark done or snooze. */
  reminder: boolean;
  /** The text to show. */
  body: string;
  /** `telegram`: a copy was also sent to Telegram. */
  delivery: 'main' | 'telegram';
  runId: string;
  firedAtMs: number;
}

export interface AutomationNoteMeta {
  kind: 'automationNote';
  automationId: string;
  automationName: string;
  /** The full text that was sent. */
  body: string;
  delivery: 'discord';
  /** Where it went, e.g. `#home`. */
  destinationLabel: string;
  /** The web session of that Discord channel. */
  destinationSessionId?: string;
  /** The message in that session. */
  destinationMessageId?: string;
  runId: string;
  firedAtMs: number;
}

export type AutomationMessageMeta = AutomationCardMeta | AutomationNoteMeta;

/** A message from either shape the clients use: a session message
 *  (`metadata`) or the console's chat message (`content.metadata`). */
export interface AutomationMessageLike {
  metadata?: unknown;
  content?: { metadata?: unknown } | null;
}

type Fields = Record<string, unknown>;

function fieldsOf(value: unknown): Fields | null {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Fields)
    : null;
}

function text(fields: Fields, key: string): string | null {
  const value = fields[key];
  return typeof value === 'string' && value !== '' ? value : null;
}

function time(fields: Fields, key: string): number | null {
  const value = fields[key];
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

/**
 * The automation card or note a message carries, or null for any other
 * message or one whose metadata is not well formed (a missing id, run, or
 * time). Unknown `delivery` values fall back to `main` for a card, so a newer
 * daemon's channel does not hide the card.
 */
export function automationMessageMeta(
  message: AutomationMessageLike,
): AutomationMessageMeta | null {
  const fields = fieldsOf(message.metadata) ?? fieldsOf(message.content?.metadata);
  if (!fields) return null;
  if (fields.kind !== 'automation' && fields.kind !== 'automationNote')
    return null;
  const automationId = text(fields, 'automationId');
  const runId = text(fields, 'runId');
  const firedAtMs = time(fields, 'firedAtMs');
  if (!automationId || !runId || firedAtMs === null) return null;
  const automationName = text(fields, 'automationName') ?? 'Automation';
  const body = typeof fields.body === 'string' ? fields.body : '';
  if (fields.kind === 'automation')
    return {
      kind: 'automation',
      automationId,
      automationName,
      reminder: fields.reminder === true,
      body,
      delivery: fields.delivery === 'telegram' ? 'telegram' : 'main',
      runId,
      firedAtMs,
    };
  if (fields.delivery !== 'discord') return null;
  const destinationSessionId = text(fields, 'destinationSessionId');
  const destinationMessageId = text(fields, 'destinationMessageId');
  return {
    kind: 'automationNote',
    automationId,
    automationName,
    body,
    delivery: 'discord',
    destinationLabel: text(fields, 'destinationLabel') ?? 'Discord',
    ...(destinationSessionId ? { destinationSessionId } : {}),
    ...(destinationMessageId ? { destinationMessageId } : {}),
    runId,
    firedAtMs,
  };
}

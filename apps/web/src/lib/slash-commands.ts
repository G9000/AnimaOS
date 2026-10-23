import type { Skill, SessionCompactInput } from '@animaos/sdk';

/** Composer slash commands (spec §15.3). `/<skill>` comes from the owner's
 *  skills (M5). */
export type SlashCommandName =
  | 'new'
  | 'clear'
  | 'reset'
  | 'stop'
  | 'rename'
  | 'archive'
  | 'export'
  | 'search'
  | 'model'
  | 'compact'
  | 'branch'
  | 'fork'
  | 'undo'
  | 'retry'
  | 'usage'
  | 'learn'
  | 'help';

export interface SlashCommand {
  /** A built-in command's name, or a skill's slug. */
  name: string;
  description: string;
  /** Set when the command needs text after its name, e.g. `a title`. */
  needs?: string;
  /** How the menu shows that text, e.g. `<title>`. */
  placeholder?: string;
  /** Set on `/<skill-slug>`: the message is sent with this skill. */
  skill?: string;
  /** A built-in instruction handled by the daemon as an ordinary task. */
  turn?: boolean;
  /** Optional arguments validated by the command itself. */
  acceptsArgument?: boolean;
}

/** A skill's description in the menu, at most this many characters. */
export const MAX_SKILL_COMMAND_DESCRIPTION = 80;
/** A `/skill` message in a Telegram session (the daemon refuses it too). */
export const SKILL_NOT_IN_TELEGRAM_CHAT =
  'Skills can’t be used in a Telegram chat.';

export const SLASH_COMMANDS: readonly (SlashCommand & {
  name: SlashCommandName;
})[] = [
  { name: 'new', description: 'Return to the main chat' },
  { name: 'stop', description: 'Stop the reply in progress' },
  {
    name: 'rename',
    description: 'Rename this chat',
    needs: 'a title',
    placeholder: '<title>',
  },
  { name: 'archive', description: 'Archive or unarchive this chat' },
  { name: 'export', description: 'Download this chat as Markdown' },
  { name: 'branch', description: 'Continue a copy of this chat' },
  { name: 'fork', description: 'Continue a copy of this chat' },
  { name: 'undo', description: 'Open a revision without the latest exchange' },
  { name: 'retry', description: 'Open a revision and edit the latest prompt' },
  {
    name: 'search',
    description: 'Search your chats',
    needs: 'words to find',
    placeholder: '<words>',
  },
  { name: 'model', description: 'Choose the model in Settings' },
  {
    name: 'compact',
    description: 'Summarize earlier messages to make room',
    placeholder: '[keep 1–100 | focus <topic>]',
    acceptsArgument: true,
  },
  {
    name: 'learn',
    description: 'Turn a workflow or source into a reusable skill',
    placeholder: '<workflow or source>',
    turn: true,
  },
  {
    name: 'clear',
    description: 'Clear the main chat; keep your companion and memories',
  },
  {
    name: 'reset',
    description: 'Clear the main chat; keep your companion and memories',
  },
  { name: 'usage', description: 'Show usage for this chat and today' },
  { name: 'help', description: 'Show every command' },
];

/** `/<slug>` for each enabled, active skill that does not shadow a
 *  built-in command (spec §15.3). */
export function skillSlashCommands(
  skills: readonly Skill[],
  builtins: readonly SlashCommand[] = SLASH_COMMANDS,
): SlashCommand[] {
  const taken = new Set(builtins.map((command) => command.name));
  return skills
    .filter(
      (skill) =>
        skill.enabled && skill.status === 'active' && !taken.has(skill.slug),
    )
    .map((skill) => ({
      name: skill.slug,
      description: skill.description.slice(0, MAX_SKILL_COMMAND_DESCRIPTION),
      placeholder: '<request>',
      skill: skill.slug,
    }));
}

export interface ParsedSlashCommand {
  command: SlashCommand;
  /** The text after the command's name, trimmed. */
  argument: string;
}

/** The command `text` starts with, or null to send it as a message: text
 *  that matches no command is an ordinary message (spec §15.3). */
export function parseSlashCommand(
  text: string,
  commands: readonly SlashCommand[] = SLASH_COMMANDS,
): ParsedSlashCommand | null {
  const match = /^\/([a-z0-9][a-z0-9-]*)(?:\s+([\s\S]*))?$/.exec(text.trim());
  if (!match) return null;
  const command = commands.find((item) => item.name === match[1]);
  return command ? { command, argument: (match[2] ?? '').trim() } : null;
}

/** The commands matching the first word while only it is typed. */
export function slashSuggestions(
  draft: string,
  commands: readonly SlashCommand[] = SLASH_COMMANDS,
): SlashCommand[] {
  const match = /^\/([a-z0-9-]*)$/.exec(draft);
  if (!match) return [];
  return commands.filter((item) => item.name.startsWith(match[1]));
}

/** What each command does in the open session; a missing handler means the
 *  command is not available there. */
export type SlashCommandHandlers = Partial<
  Record<SlashCommandName, (argument: string) => void>
>;

/** Runs a built-in command: null when it ran, otherwise why it could not.
 *  A skill is sent as a message by the caller, never run here. */
export function runSlashCommand(
  parsed: ParsedSlashCommand,
  handlers: SlashCommandHandlers,
): string | null {
  const { command, argument } = parsed;
  const handler = command.skill
    ? undefined
    : handlers[command.name as SlashCommandName];
  if (!handler) return `/${command.name} is not available here.`;
  if (command.needs && !argument)
    return `Add ${command.needs} after /${command.name}.`;
  // An argument-less command with text after it (S3b-D): refused, not
  // silently ignored, so the extra text is never discarded.
  if (!command.needs && !command.acceptsArgument && argument)
    return `/${command.name} takes no text.`;
  if (command.name === 'compact') {
    try {
      parseCompactOptions(argument);
    } catch (error) {
      return error instanceof Error ? error.message : String(error);
    }
  }
  handler(argument);
  return null;
}

/** Keep parsing local so invalid commands remain visible in the composer. */
export function parseCompactOptions(argument: string): SessionCompactInput {
  const text = argument.trim();
  if (!text) return {};
  const keep = /^(?:keep|here)\s+(\d+)$/.exec(text);
  if (keep) {
    const count = Number(keep[1]);
    if (Number.isSafeInteger(count) && count >= 1 && count <= 100)
      return { keepLatestExchanges: count };
  }
  const focus = /^focus\s+([\s\S]+)$/.exec(text)?.[1].trim();
  if (focus && Array.from(focus).length <= 500) return { focus };
  throw new Error(
    'Use /compact, /compact keep 1–100, or /compact focus <topic up to 500 characters>.',
  );
}

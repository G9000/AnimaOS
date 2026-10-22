import type { SlashCommand } from './components/input-bar.js';

const SWARM_SLASH_COMMANDS: SlashCommand[] = [
  { name: 'agents', description: 'browse and edit agents' },
  { name: 'history', description: 'browse past runs' },
  { name: 'resume', description: 'browse saved runs or resume by label' },
  { name: 'rename', description: 'name the current saved run' },
  {
    name: 'delete',
    description: 'delete a saved run by label',
    args: '<label>',
  },
  { name: 'undo', description: 'restore the last deleted saved run' },
  { name: 'undo-drop', description: 'discard the oldest queued undo' },
  { name: 'undo-status', description: 'show deleted-run undo queue' },
  { name: 'trace', description: 'inspect messages and tool activity' },
  { name: 'result', description: 'view the full last result' },
  { name: 'status', description: 'show current session state' },
  { name: 'health', description: 'check current daemon connectivity' },
  { name: 'retry', description: 'rerun the last task' },
  { name: 'help', description: 'show available commands' },
  { name: 'clear', description: 'clear session history' },
  { name: 'exit', description: 'exit the session' },
];

const RESULT_BASE_COMMANDS: SlashCommand[] = [
  { name: 'back', description: 'return to swarm view' },
  {
    name: 'resume',
    description: 'browse saved runs to resume',
  },
  {
    name: 'rename',
    description: 'name the current saved run',
    args: '<label>',
  },
  {
    name: 'delete',
    description: 'delete the current saved run',
  },
  {
    name: 'undo',
    description: 'restore the last deleted saved run',
  },
  {
    name: 'undo-drop',
    description: 'discard the oldest queued undo',
  },
  {
    name: 'undo-status',
    description: 'show deleted-run undo queue',
  },
];

const RETRY_COMMAND: SlashCommand = {
  name: 'retry',
  description: 'rerun the last task',
};

const HEALTH_COMMANDS: SlashCommand[] = [
  {
    name: 'health',
    description: 'check current daemon connectivity',
  },
  {
    name: 'help',
    description: 'show available commands',
  },
];

const SESSION_NAVIGATION_COMMANDS: SlashCommand[] = [
  { name: 'chat', description: 'open the current or most recent chat session' },
  { name: 'sessions', description: 'browse chat sessions' },
  { name: 'new', description: 'create a chat session' },
  {
    name: 'open',
    description: 'open a chat session by ID or title',
    args: '<session-id-or-title>',
  },
];

const SESSION_AGENT_COMMAND: SlashCommand = {
  name: 'agent',
  description: 'choose the chat agent',
};
const SESSION_QUEUE_COMMAND: SlashCommand = {
  name: 'queue',
  description: 'queue the next message',
};
const SESSION_STEER_COMMAND: SlashCommand = {
  name: 'steer',
  description: 'steer the active run with the next message',
};

function withoutRetryCommand(commands: SlashCommand[]): SlashCommand[] {
  return commands.filter((command) => command.name !== 'retry');
}

export interface BuildAppInputCommandsOptions {
  interactive: boolean;
  hasSavedRuns: boolean;
  taskEntryBlockedByDaemon: boolean;
  showResultCommands: boolean;
  sessionView?: 'picker' | 'chat' | null;
  hasMultipleAgents?: boolean;
  canSteerSession?: boolean;
  canStopSessionRun?: boolean;
}

export interface AppInputCommands {
  swarmInputCommands: SlashCommand[];
  resultInputCommands: SlashCommand[];
  sessionPickerInputCommands: SlashCommand[];
  sessionChatInputCommands: SlashCommand[];
  helpCommands: SlashCommand[];
}

export function buildAppInputCommands({
  interactive,
  hasSavedRuns,
  taskEntryBlockedByDaemon,
  showResultCommands,
  sessionView = null,
  hasMultipleAgents = false,
  canSteerSession = false,
  canStopSessionRun = false,
}: BuildAppInputCommandsOptions): AppInputCommands {
  const baseSwarmCommands = taskEntryBlockedByDaemon
    ? withoutRetryCommand(SWARM_SLASH_COMMANDS)
    : SWARM_SLASH_COMMANDS;
  const sessionNavigationCommands = [
    ...SESSION_NAVIGATION_COMMANDS,
    ...(hasMultipleAgents ? [SESSION_AGENT_COMMAND] : []),
  ];
  const swarmInputCommands = interactive
    ? [...baseSwarmCommands, ...sessionNavigationCommands]
    : baseSwarmCommands;
  const resultInputCommands = [
    RESULT_BASE_COMMANDS[0],
    ...(hasSavedRuns ? RESULT_BASE_COMMANDS.slice(1) : []),
    ...(interactive && !taskEntryBlockedByDaemon ? [RETRY_COMMAND] : []),
    ...(taskEntryBlockedByDaemon ? HEALTH_COMMANDS : []),
  ];
  const sessionPickerInputCommands =
    interactive && sessionView === 'picker'
      ? [
          ...sessionNavigationCommands,
          { name: 'back', description: 'return to the previous view' },
          { name: 'help', description: 'show available commands' },
        ]
      : [];
  const sessionChatInputCommands =
    interactive && sessionView === 'chat'
      ? [
          ...sessionNavigationCommands,
          SESSION_QUEUE_COMMAND,
          {
            name: 'review',
            description: 'review pending approvals and clarifications',
          },
          { name: 'approvals', description: 'review pending tool approvals' },
          {
            name: 'clarifications',
            description: 'answer pending clarification questions',
          },
          ...(canSteerSession ? [SESSION_STEER_COMMAND] : []),
          { name: 'back', description: 'return to the previous view' },
          ...(canStopSessionRun
            ? [{ name: 'stop', description: 'stop the active run' }]
            : []),
          { name: 'help', description: 'show available commands' },
        ]
      : [];

  return {
    swarmInputCommands,
    resultInputCommands,
    sessionPickerInputCommands,
    sessionChatInputCommands,
    helpCommands:
      sessionView === 'chat'
        ? sessionChatInputCommands
        : sessionView === 'picker'
          ? sessionPickerInputCommands
          : showResultCommands
            ? resultInputCommands
            : swarmInputCommands,
  };
}

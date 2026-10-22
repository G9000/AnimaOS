import { describe, expect, it } from 'vitest';
import { buildAppInputCommands } from './input-commands.js';

function commandNames(commands: Array<{ name: string }>): string[] {
  return commands.map((command) => command.name);
}

describe('input command helpers', () => {
  it('shows only session entry commands in the interactive swarm view', () => {
    const commands = buildAppInputCommands({
      interactive: true,
      hasSavedRuns: false,
      taskEntryBlockedByDaemon: false,
      showResultCommands: false,
      hasMultipleAgents: true,
    });

    expect(commandNames(commands.swarmInputCommands)).toEqual(
      expect.arrayContaining([
        'chat',
        'sessions',
        'new',
        'open',
        'agent',
        'agents',
        'history',
        'trace',
      ])
    );
    for (const name of ['queue', 'steer', 'stop']) {
      expect(commandNames(commands.swarmInputCommands)).not.toContain(name);
    }
  });

  it('hides agent selection when there is only one agent', () => {
    const commands = buildAppInputCommands({
      interactive: true,
      hasSavedRuns: false,
      taskEntryBlockedByDaemon: false,
      showResultCommands: false,
    });

    expect(commandNames(commands.swarmInputCommands)).not.toContain('agent');
  });

  it('limits session picker commands to navigation and session selection', () => {
    const commands = buildAppInputCommands({
      interactive: true,
      hasSavedRuns: false,
      taskEntryBlockedByDaemon: false,
      showResultCommands: false,
      sessionView: 'picker',
      hasMultipleAgents: true,
    });

    expect(commandNames(commands.sessionPickerInputCommands)).toEqual([
      'chat',
      'sessions',
      'new',
      'open',
      'agent',
      'back',
      'help',
    ]);
    expect(commandNames(commands.sessionPickerInputCommands)).not.toContain(
      'stop'
    );
    expect(commandNames(commands.sessionPickerInputCommands)).not.toContain(
      'queue'
    );
  });

  it('shows run controls in chat only when there is a stoppable run', () => {
    const withoutRun = buildAppInputCommands({
      interactive: true,
      hasSavedRuns: false,
      taskEntryBlockedByDaemon: false,
      showResultCommands: false,
      sessionView: 'chat',
      hasMultipleAgents: true,
      canSteerSession: true,
    });
    const withRun = buildAppInputCommands({
      interactive: true,
      hasSavedRuns: false,
      taskEntryBlockedByDaemon: false,
      showResultCommands: false,
      sessionView: 'chat',
      hasMultipleAgents: true,
      canSteerSession: true,
      canStopSessionRun: true,
    });

    expect(commandNames(withoutRun.sessionChatInputCommands)).toEqual(
      expect.arrayContaining([
        'chat',
        'sessions',
        'new',
        'open',
        'agent',
        'queue',
        'steer',
        'back',
        'help',
      ])
    );
    expect(commandNames(withoutRun.sessionChatInputCommands)).not.toContain(
      'stop'
    );
    expect(commandNames(withRun.sessionChatInputCommands)).toContain('stop');
  });

  it('hides steer when the active session does not support it', () => {
    const commands = buildAppInputCommands({
      interactive: true,
      hasSavedRuns: false,
      taskEntryBlockedByDaemon: false,
      showResultCommands: false,
      sessionView: 'chat',
      hasMultipleAgents: false,
      canSteerSession: false,
      canStopSessionRun: false,
    });

    expect(commandNames(commands.sessionChatInputCommands)).not.toContain(
      'agent'
    );
    expect(commandNames(commands.sessionChatInputCommands)).not.toContain(
      'steer'
    );
    expect(commandNames(commands.sessionChatInputCommands)).not.toContain(
      'stop'
    );
  });

  it('does not expose session commands in non-interactive mode', () => {
    const commands = buildAppInputCommands({
      interactive: false,
      hasSavedRuns: false,
      taskEntryBlockedByDaemon: false,
      showResultCommands: false,
      sessionView: 'chat',
      canStopSessionRun: true,
    });

    expect(commands.sessionPickerInputCommands).toEqual([]);
    expect(commands.sessionChatInputCommands).toEqual([]);
    expect(commandNames(commands.swarmInputCommands)).not.toContain('chat');
  });

  it('removes retry and exposes health commands when task entry is blocked', () => {
    const commands = buildAppInputCommands({
      interactive: true,
      hasSavedRuns: true,
      taskEntryBlockedByDaemon: true,
      showResultCommands: true,
    });

    expect(commandNames(commands.swarmInputCommands)).not.toContain('retry');
    expect(commandNames(commands.resultInputCommands)).toEqual([
      'back',
      'resume',
      'rename',
      'delete',
      'undo',
      'undo-drop',
      'undo-status',
      'health',
      'help',
    ]);
    expect(commandNames(commands.helpCommands)).toEqual(
      commandNames(commands.resultInputCommands)
    );
  });

  it('keeps retry available in interactive healthy sessions', () => {
    const commands = buildAppInputCommands({
      interactive: true,
      hasSavedRuns: true,
      taskEntryBlockedByDaemon: false,
      showResultCommands: true,
    });

    expect(commandNames(commands.swarmInputCommands)).toContain('retry');
    expect(commandNames(commands.resultInputCommands)).toContain('retry');
    expect(commandNames(commands.resultInputCommands)).not.toContain('health');
  });

  it('limits result commands to back when nothing can be resumed or retried', () => {
    const commands = buildAppInputCommands({
      interactive: false,
      hasSavedRuns: false,
      taskEntryBlockedByDaemon: false,
      showResultCommands: true,
    });

    expect(commandNames(commands.resultInputCommands)).toEqual(['back']);
  });
});

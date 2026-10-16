import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DaemonHttpError } from '@animaOS-SWARM/sdk';

import type { AutomationsView } from '../hooks/useAutomations';
import {
  AGENT_CREATED_NOTE,
  PHRASE_NOT_UNDERSTOOD,
  localTimeZone,
} from '../lib/automations';
import { daemon } from '../lib/daemon-api';
import { automationFixture, automationRunFixture } from '../test/automations';
import { AutomationsPage } from './AutomationsPage';

function fakeView(overrides: Partial<AutomationsView> = {}): AutomationsView {
  return {
    automations: [],
    loaded: true,
    error: null,
    errorStatus: null,
    refresh: vi.fn(),
    create: vi.fn().mockResolvedValue(true),
    createHeartbeat: vi.fn().mockResolvedValue(true),
    update: vi.fn().mockResolvedValue(true),
    setEnabled: vi.fn().mockResolvedValue(true),
    remove: vi.fn().mockResolvedValue(true),
    undo: vi.fn().mockResolvedValue(true),
    runNow: vi.fn().mockResolvedValue(true),
    ...overrides,
  };
}

function renderPage(
  view: AutomationsView,
  extra: { focusId?: string; online?: boolean } = {},
) {
  const onOpenSession = vi.fn();
  const onFocusHandled = vi.fn();
  render(
    <AutomationsPage
      view={view}
      agentId="agent-main"
      version={0}
      online={extra.online ?? true}
      telegramConnectorId={null}
      focusId={extra.focusId ?? null}
      onFocusHandled={onFocusHandled}
      onOpenSession={onOpenSession}
    />,
  );
  return { onOpenSession, onFocusHandled };
}

const agentMade = automationFixture('schedule-1', {
  name: '<b>Stretch</b>',
  prompt: '**Remind** me to stretch',
  activeHours: {
    start: '08:00',
    end: '22:00',
    days: [1, 2, 3, 4, 5],
    timeZone: 'UTC',
  },
  lastOutcome: {
    status: 'error',
    occurredAtMs: 5,
    errorCode: 'schedule_run_failed',
  },
  lastFiredAtMs: 5,
  counters: { runs: 3, failures: 2, consecutiveFailures: 2 },
  createdBy: {
    kind: 'agent',
    agentId: 'agent-main',
    sessionId: 'chat:1',
    runId: 'run_1',
    toolCallId: 'call-1',
  },
});
const heartbeat = automationFixture('schedule-2', {
  name: 'Heartbeat',
  preset: 'heartbeat',
  enabled: false,
});

const interrupted = automationFixture('schedule-interrupted', {
  name: 'Interrupted check-in',
  enabled: false,
  lastFiredAtMs: 5,
  lastOutcome: {
    status: 'error',
    occurredAtMs: 5,
    errorCode: 'schedule_run_interrupted',
  },
});

function expectNoAutomationMutation(view: AutomationsView) {
  for (const method of [
    view.runNow,
    view.setEnabled,
    view.create,
    view.createHeartbeat,
    view.update,
    view.remove,
    view.undo,
  ]) {
    expect(method).not.toHaveBeenCalled();
  }
}

beforeEach(() => {
  // Settled only by the tests that read them, so no update lands after a
  // test ends.
  vi.spyOn(daemon, 'previewAutomation').mockReturnValue(new Promise(() => {}));
  vi.spyOn(daemon, 'automationHistory').mockReturnValue(new Promise(() => {}));
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('AutomationsPage', () => {
  it('m76 reviews an interrupted occurrence without running, resuming, or inventing a thread', async () => {
    const user = userEvent.setup();
    vi.mocked(daemon.automationHistory).mockResolvedValue([
      automationRunFixture('interrupted-occurrence', {
        scheduleId: interrupted.id,
        outcome: 'failed',
        errorCode: 'schedule_run_interrupted',
        sessionId: 'saved-session:actual-occurrence',
      }),
    ]);
    const view = fakeView({ automations: [interrupted] });
    const { onOpenSession } = renderPage(view);
    const row = screen.getByRole('article', { name: interrupted.name });
    expect(
      within(row).queryByRole('button', { name: 'Open thread', exact: true }),
    ).not.toBeInTheDocument();
    expect(within(row).getByText('Needs review')).toBeVisible();
    expect(
      within(row).getByText(/may have completed some effects/i),
    ).toBeVisible();
    expect(
      within(row).getByText(/review.*thread.*effects.*run now.*resume/i),
    ).toBeVisible();
    expectNoAutomationMutation(view);

    await user.click(
      within(row).getByRole('button', { name: 'Review interrupted run' }),
    );
    const drawer = screen.getByRole('complementary', {
      name: `History of ${interrupted.name}`,
    });
    const review = await within(drawer).findByRole('button', {
      name: 'Review thread',
    });
    expect(daemon.automationHistory).toHaveBeenCalledTimes(1);
    expect(daemon.automationHistory).toHaveBeenCalledWith(
      'agent-main',
      interrupted.id,
      {
        signal: expect.any(AbortSignal),
      },
    );
    expect(onOpenSession).not.toHaveBeenCalled();
    expectNoAutomationMutation(view);
    await user.click(review);
    expect(onOpenSession).toHaveBeenCalledTimes(1);
    expect(onOpenSession).toHaveBeenCalledWith(
      'saved-session:actual-occurrence',
    );
    expectNoAutomationMutation(view);
  });

  it('m76 does not infer a thread when an interrupted occurrence has no saved session', async () => {
    const user = userEvent.setup();
    vi.mocked(daemon.automationHistory).mockResolvedValue([
      automationRunFixture('interrupted-without-session', {
        scheduleId: interrupted.id,
        outcome: 'failed',
        errorCode: 'schedule_run_interrupted',
        sessionId: null,
      }),
    ]);
    const view = fakeView({ automations: [interrupted] });
    const { onOpenSession } = renderPage(view);
    const row = screen.getByRole('article', { name: interrupted.name });
    await user.click(
      within(row).getByRole('button', { name: 'Review interrupted run' }),
    );
    const drawer = screen.getByRole('complementary', {
      name: `History of ${interrupted.name}`,
    });
    expect(
      await within(drawer).findByText('No saved thread is available.'),
    ).toBeVisible();
    expect(
      within(row).queryByRole('button', { name: 'Open thread', exact: true }),
    ).not.toBeInTheDocument();
    expect(
      within(drawer).queryByRole('button', {
        name: 'Review thread',
        exact: true,
      }),
    ).not.toBeInTheDocument();
    expect(
      within(drawer).queryByRole('button', { name: 'Open', exact: true }),
    ).not.toBeInTheDocument();
    expect(onOpenSession).not.toHaveBeenCalled();
    expectNoAutomationMutation(view);
    await user.click(
      within(drawer).getByRole('button', { name: 'Close history' }),
    );
    expect(onOpenSession).not.toHaveBeenCalled();
    expectNoAutomationMutation(view);
  });

  it('m76 keeps Run now and Resume as separate explicit actions after interruption guidance', async () => {
    const user = userEvent.setup();
    const view = fakeView({ automations: [interrupted] });
    renderPage(view);
    const row = screen.getByRole('article', { name: interrupted.name });
    expect(within(row).getByText('Needs review')).toBeVisible();
    expectNoAutomationMutation(view);
    await user.click(
      within(row).getByRole('button', { name: 'Run now', exact: true }),
    );
    expect(view.runNow).toHaveBeenCalledTimes(1);
    expect(view.runNow).toHaveBeenCalledWith(interrupted);
    expect(view.setEnabled).not.toHaveBeenCalled();
    await user.click(
      within(row).getByRole('button', { name: 'Resume', exact: true }),
    );
    expect(view.setEnabled).toHaveBeenCalledTimes(1);
    expect(view.setEnabled).toHaveBeenCalledWith(interrupted, true);
    expect(view.runNow).toHaveBeenCalledTimes(1);
    expect(view.update).not.toHaveBeenCalled();
    expect(daemon.automationHistory).not.toHaveBeenCalled();
  });

  it('m76 keeps ordinary failure and stopped outcomes outside interruption review', () => {
    const ordinaryFailure = automationFixture('ordinary-failure', {
      lastOutcome: {
        status: 'error',
        occurredAtMs: 5,
        errorCode: 'schedule_run_failed',
      },
    });
    const stopped = automationFixture('owner-stopped', {
      lastOutcome: {
        status: 'stopped',
        occurredAtMs: 5,
        errorCode: 'schedule_run_interrupted',
      },
    });
    const view = fakeView({ automations: [ordinaryFailure, stopped] });
    renderPage(view);
    expect(
      within(
        screen.getByRole('article', { name: ordinaryFailure.name }),
      ).getByText(/Last: Failed/),
    ).toBeVisible();
    expect(
      within(screen.getByRole('article', { name: stopped.name })).getByText(
        /Last: Stopped/,
      ),
    ).toBeVisible();
    expect(screen.queryByText('Needs review')).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Review interrupted run' }),
    ).not.toBeInTheDocument();
    expect(
      screen.getAllByRole('button', { name: 'Run now', exact: true }),
    ).toHaveLength(2);
    expectNoAutomationMutation(view);
  });

  it('m76 allows offline review entry without history requests or execution', async () => {
    const user = userEvent.setup();
    const view = fakeView({ automations: [interrupted] });
    const { onOpenSession } = renderPage(view, { online: false });
    const row = screen.getByRole('article', { name: interrupted.name });
    expect(within(row).getByText('Needs review')).toBeVisible();
    await user.click(
      within(row).getByRole('button', { name: 'Review interrupted run' }),
    );
    const drawer = screen.getByRole('complementary', {
      name: `History of ${interrupted.name}`,
    });
    expect(drawer).toBeVisible();
    expect(daemon.automationHistory).not.toHaveBeenCalled();
    expect(
      within(row).getByRole('button', { name: 'Run now', exact: true }),
    ).toBeDisabled();
    expect(
      within(row).getByRole('button', { name: 'Resume', exact: true }),
    ).toBeDisabled();
    await user.click(
      within(row).getByRole('button', { name: 'Run now', exact: true }),
    );
    await user.click(
      within(row).getByRole('button', { name: 'Resume', exact: true }),
    );
    await user.click(
      within(drawer).getByRole('button', { name: 'Close history' }),
    );
    expect(screen.queryByRole('complementary')).not.toBeInTheDocument();
    expect(onOpenSession).not.toHaveBeenCalled();
    expectNoAutomationMutation(view);
  });

  it('lists automations with their schedule, outcome, and creator as text', () => {
    renderPage(fakeView({ automations: [agentMade, heartbeat] }));

    const row = screen.getByRole('article', { name: '<b>Stretch</b>' });
    expect(within(row).getByText('<b>Stretch</b>')).toBeInTheDocument();
    expect(
      within(row).getByText('**Remind** me to stretch'),
    ).toBeInTheDocument();
    expect(
      within(row).getByText('every 30 min · 08:00–22:00, weekdays (UTC)'),
    ).toBeInTheDocument();
    expect(within(row).getByText(/Last: Failed/)).toBeInTheDocument();
    expect(within(row).getByText(/2 failed in a row/)).toBeInTheDocument();
    expect(within(row).getByText(AGENT_CREATED_NOTE)).toBeInTheDocument();
    const paused = screen.getByRole('article', { name: 'Heartbeat' });
    expect(within(paused).getAllByText('Paused').length).toBeGreaterThan(0);
    expect(
      screen.getByRole('button', { name: 'Add heartbeat' }),
    ).toBeDisabled();
    expect(document.querySelector('.automations-page strong')).toBeNull();
  });

  it('shows invisible characters of a companion-written name and prompt as markers', async () => {
    const user = userEvent.setup();
    // U+202E (direction override) and U+200B (zero width space), built from
    // code points so no invisible character sits in this file.
    const override = String.fromCodePoint(0x202e);
    const zeroWidth = String.fromCodePoint(0x200b);
    const sneaky = automationFixture('schedule-3', {
      name: `Backup${override}`,
      prompt: `Run${zeroWidth}backup`,
    });
    renderPage(fakeView({ automations: [sneaky] }));

    const row = screen.getByRole('article', { name: 'Backup⟨U+202E⟩' });
    expect(within(row).getByText('Backup⟨U+202E⟩')).toBeInTheDocument();
    expect(within(row).getByText('Run⟨U+200B⟩backup')).toBeInTheDocument();
    expect(row.textContent).not.toContain(override);
    expect(row.textContent).not.toContain(zeroWidth);

    await user.click(within(row).getByRole('button', { name: 'Edit' }));
    const form = screen.getByRole('form', { name: 'Edit Backup⟨U+202E⟩' });
    expect(
      within(form).getByText('This text contains 2 invisible characters'),
    ).toBeInTheDocument();
  });

  it('runs, pauses, resumes, opens the thread, and deletes after confirming', async () => {
    const user = userEvent.setup();
    const view = fakeView({ automations: [agentMade, heartbeat] });
    const { onOpenSession } = renderPage(view);
    const row = screen.getByRole('article', { name: '<b>Stretch</b>' });

    await user.click(within(row).getByRole('button', { name: 'Run now' }));
    expect(view.runNow).toHaveBeenCalledWith(agentMade);
    await user.click(within(row).getByRole('button', { name: 'Pause' }));
    expect(view.setEnabled).toHaveBeenCalledWith(agentMade, false);
    await user.click(
      within(screen.getByRole('article', { name: 'Heartbeat' })).getByRole(
        'button',
        { name: 'Resume' },
      ),
    );
    expect(view.setEnabled).toHaveBeenCalledWith(heartbeat, true);
    await user.click(within(row).getByRole('button', { name: 'Open thread' }));
    expect(onOpenSession).toHaveBeenCalledWith('schedule:schedule-1');

    await user.click(within(row).getByRole('button', { name: 'Delete' }));
    expect(view.remove).not.toHaveBeenCalled();
    await user.click(
      within(row).getByRole('button', { name: 'Confirm delete' }),
    );
    expect(view.remove).toHaveBeenCalledWith(agentMade);
  });

  it('creates an automation from a phrase with the daemon preview', async () => {
    const user = userEvent.setup();
    const preview = vi
      .mocked(daemon.previewAutomation)
      .mockResolvedValue([
        Date.UTC(2026, 0, 5, 10),
        Date.UTC(2026, 0, 5, 12),
        Date.UTC(2026, 0, 5, 14),
      ]);
    const view = fakeView();
    renderPage(view);

    await user.click(screen.getByRole('button', { name: 'New automation' }));
    const form = screen.getByRole('form', { name: 'New automation' });
    await user.type(within(form).getByLabelText('Name'), 'Stretch');
    await user.type(
      within(form).getByLabelText('Prompt'),
      'Remind me to stretch',
    );
    await user.type(within(form).getByLabelText('When'), 'every 2 hours');

    const runs = await within(form).findByRole('region', { name: 'Next runs' });
    expect(within(runs).getAllByRole('listitem')).toHaveLength(3);
    expect(preview).toHaveBeenLastCalledWith(
      { trigger: { type: 'interval', intervalMs: 7_200_000 } },
      { signal: expect.any(AbortSignal) },
    );
    await user.click(
      within(form).getByRole('button', { name: 'Create automation' }),
    );

    expect(view.create).toHaveBeenCalledWith({
      prompt: 'Remind me to stretch',
      trigger: { type: 'interval', intervalMs: 7_200_000 },
      target: { type: 'workspace' },
      name: 'Stretch',
    });
    await waitFor(() =>
      expect(screen.queryByRole('form', { name: 'New automation' })).toBeNull(),
    );
  });

  it('explains an unknown phrase and shows the daemon problem with a cron expression', async () => {
    const user = userEvent.setup();
    renderPage(fakeView());
    await user.click(screen.getByRole('button', { name: 'New automation' }));
    const form = screen.getByRole('form', { name: 'New automation' });
    await user.type(within(form).getByLabelText('Prompt'), 'Check');
    await user.type(within(form).getByLabelText('When'), 'whenever');

    expect(within(form).getByText(PHRASE_NOT_UNDERSTOOD)).toBeInTheDocument();
    expect(
      within(form).getByRole('button', { name: 'Create automation' }),
    ).toBeDisabled();

    vi.mocked(daemon.previewAutomation).mockRejectedValue(
      new DaemonHttpError(400, { error: 'minute: must be from 0 to 59' }),
    );
    await user.click(within(form).getByLabelText('Use a cron expression'));
    await user.type(
      within(form).getByLabelText('Cron expression'),
      '61 * * * *',
    );

    expect(
      await within(form).findByText('minute: must be from 0 to 59'),
    ).toBeInTheDocument();
  });

  it('sends active hours with a new automation', async () => {
    const user = userEvent.setup();
    vi.mocked(daemon.previewAutomation).mockResolvedValue([10]);
    const view = fakeView();
    renderPage(view);
    await user.click(screen.getByRole('button', { name: 'New automation' }));
    const form = screen.getByRole('form', { name: 'New automation' });
    await user.type(within(form).getByLabelText('Prompt'), 'Check');
    await user.type(within(form).getByLabelText('When'), 'every hour');
    await user.click(within(form).getByLabelText('Only run between'));
    await user.click(within(form).getByLabelText('Sun'));
    await user.click(within(form).getByLabelText('Sat'));
    await within(form).findByRole('region', { name: 'Next runs' });
    await user.click(
      within(form).getByRole('button', { name: 'Create automation' }),
    );

    expect(view.create).toHaveBeenCalledWith({
      prompt: 'Check',
      trigger: { type: 'interval', intervalMs: 3_600_000 },
      target: { type: 'workspace' },
      activeHours: {
        start: '08:00',
        end: '22:00',
        days: [1, 2, 3, 4, 5],
        timeZone: localTimeZone(),
      },
    });
  });

  it('edits without touching the schedule and can clear active hours', async () => {
    const user = userEvent.setup();
    const view = fakeView({ automations: [agentMade] });
    renderPage(view);
    await user.click(
      within(screen.getByRole('article', { name: '<b>Stretch</b>' })).getByRole(
        'button',
        { name: 'Edit' },
      ),
    );
    const form = screen.getByRole('form', { name: 'Edit <b>Stretch</b>' });
    const prompt = within(form).getByLabelText('Prompt');
    await user.clear(prompt);
    await user.type(prompt, 'Stand up');
    await user.click(within(form).getByLabelText('Only run between'));
    await user.click(
      within(form).getByRole('button', { name: 'Save changes' }),
    );

    // The target and schedule were not touched, so neither is sent.
    expect(view.update).toHaveBeenCalledWith(agentMade, {
      name: '<b>Stretch</b>',
      prompt: 'Stand up',
      activeHours: null,
    });
  });

  it('keeps the target of a Telegram automation when only the prompt changes', async () => {
    const user = userEvent.setup();
    const onTelegram = automationFixture('schedule-4', {
      name: 'Digest',
      target: { type: 'connector', connectorId: 'connector-old' },
    });
    const view = fakeView({ automations: [onTelegram] });
    renderPage(view);
    await user.click(
      within(screen.getByRole('article', { name: 'Digest' })).getByRole(
        'button',
        { name: 'Edit' },
      ),
    );
    const form = screen.getByRole('form', { name: 'Edit Digest' });
    const prompt = within(form).getByLabelText('Prompt');
    await user.clear(prompt);
    await user.type(prompt, 'New text');
    await user.click(
      within(form).getByRole('button', { name: 'Save changes' }),
    );

    expect(view.update).toHaveBeenCalledTimes(1);
    const patch = vi.mocked(view.update).mock.calls[0][1];
    expect(patch).toEqual({
      name: 'Digest',
      prompt: 'New text',
      activeHours: null,
    });
    expect('target' in patch).toBe(false);
  });

  it('sends the target once the owner picks one', async () => {
    const user = userEvent.setup();
    const onTelegram = automationFixture('schedule-4', {
      name: 'Digest',
      target: { type: 'connector', connectorId: 'connector-old' },
    });
    const view = fakeView({ automations: [onTelegram] });
    renderPage(view);
    await user.click(
      within(screen.getByRole('article', { name: 'Digest' })).getByRole(
        'button',
        { name: 'Edit' },
      ),
    );
    const form = screen.getByRole('form', { name: 'Edit Digest' });
    await user.selectOptions(
      within(form).getByLabelText('Runs in'),
      'workspace',
    );
    await user.click(
      within(form).getByRole('button', { name: 'Save changes' }),
    );

    expect(view.update).toHaveBeenCalledWith(
      onTelegram,
      expect.objectContaining({ target: { type: 'workspace' } }),
    );
  });

  it('leaves an empty name out of an edit', async () => {
    const user = userEvent.setup();
    const view = fakeView({ automations: [agentMade] });
    renderPage(view);
    await user.click(
      within(screen.getByRole('article', { name: '<b>Stretch</b>' })).getByRole(
        'button',
        { name: 'Edit' },
      ),
    );
    const form = screen.getByRole('form', { name: 'Edit <b>Stretch</b>' });
    await user.clear(within(form).getByLabelText('Name'));
    await user.click(
      within(form).getByRole('button', { name: 'Save changes' }),
    );

    const patch = vi.mocked(view.update).mock.calls[0][1];
    expect('name' in patch).toBe(false);
    expect(patch.prompt).toBe('**Remind** me to stretch');
  });

  it('does not crash on a half-typed time zone', async () => {
    const user = userEvent.setup();
    renderPage(fakeView());
    await user.click(screen.getByRole('button', { name: 'New automation' }));
    const form = screen.getByRole('form', { name: 'New automation' });
    await user.type(within(form).getByLabelText('Prompt'), 'Check');
    await user.type(within(form).getByLabelText('When'), 'tomorrow at 9am');
    const zone = within(form).getByLabelText('Time zone');
    await user.clear(zone);
    expect(within(form).getByText(PHRASE_NOT_UNDERSTOOD)).toBeInTheDocument();
    await user.type(zone, 'Europe/Lon');

    expect(screen.getByRole('form', { name: 'New automation' })).toBeVisible();
    expect(
      within(form).getByRole('button', { name: 'Create automation' }),
    ).toBeDisabled();
  });

  it('saves a new schedule only once the daemon has shown its runs', async () => {
    const user = userEvent.setup();
    const preview = vi.mocked(daemon.previewAutomation);
    renderPage(fakeView());
    await user.click(screen.getByRole('button', { name: 'New automation' }));
    const form = screen.getByRole('form', { name: 'New automation' });
    const create = () =>
      within(form).getByRole('button', { name: 'Create automation' });
    const when = within(form).getByLabelText('When');
    await user.type(within(form).getByLabelText('Prompt'), 'Check');
    await user.type(when, 'every hour');
    // The preview is still loading.
    expect(create()).toBeDisabled();

    preview.mockRejectedValue(new DaemonHttpError(400, { error: 'Too soon' }));
    await user.clear(when);
    await user.type(when, 'every 5 minutes');
    expect(await within(form).findByText('Too soon')).toBeInTheDocument();
    expect(create()).toBeDisabled();

    preview.mockResolvedValue([10]);
    await user.clear(when);
    await user.type(when, 'every 6 minutes');
    await within(form).findByRole('region', { name: 'Next runs' });
    expect(create()).toBeEnabled();
  });

  it('adds the heartbeat in the browser time zone', async () => {
    const user = userEvent.setup();
    const view = fakeView();
    renderPage(view);
    await user.click(screen.getByRole('button', { name: 'Add heartbeat' }));
    expect(view.createHeartbeat).toHaveBeenCalledWith(localTimeZone());
  });

  it('shows the latest runs in the history drawer', async () => {
    const user = userEvent.setup();
    vi.mocked(daemon.automationHistory).mockResolvedValue([
      automationRunFixture('run-a', {
        outcome: 'failed',
        manual: true,
        errorCode: 'schedule_run_failed',
      }),
    ]);
    renderPage(fakeView({ automations: [agentMade] }));
    await user.click(
      within(screen.getByRole('article', { name: '<b>Stretch</b>' })).getByRole(
        'button',
        { name: 'History' },
      ),
    );

    const drawer = screen.getByRole('complementary', {
      name: 'History of <b>Stretch</b>',
    });
    expect(await within(drawer).findByText('Failed')).toBeInTheDocument();
    expect(within(drawer).getByText('Run now')).toBeInTheDocument();
    expect(within(drawer).getByText('schedule_run_failed')).toBeInTheDocument();
    expect(daemon.automationHistory).toHaveBeenCalledWith(
      'agent-main',
      'schedule-1',
      { signal: expect.any(AbortSignal) },
    );
    await user.click(
      within(drawer).getByRole('button', { name: 'Close history' }),
    );
    expect(screen.queryByRole('complementary')).toBeNull();
  });

  it('opens the editor of a focused automation', () => {
    const { onFocusHandled } = renderPage(
      fakeView({ automations: [agentMade] }),
      {
        focusId: 'schedule-1',
      },
    );
    expect(
      screen.getByRole('form', { name: 'Edit <b>Stretch</b>' }),
    ).toBeInTheDocument();
    expect(onFocusHandled).toHaveBeenCalledTimes(1);
  });
});

import { useEffect, useState } from 'react';
import type { Automation } from '@animaOS-SWARM/sdk';

import {
  AutomationEditor,
  type AutomationDraft,
} from '../components/automations/AutomationEditor';
import {
  AutomationHistory,
  type ShowInChat,
} from '../components/automations/AutomationHistory';
import type { AutomationsView } from '../hooks/useAutomations';
import { formatWhen } from '../lib/approvals';
import {
  destinationChips,
  groupAutomations,
  isInterrupted,
  type DestinationChip,
} from '../lib/automation-cards';
import {
  AGENT_CREATED_NOTE,
  OUTCOME_LABELS,
  describeActiveHours,
  describeTrigger,
  localTimeZone,
} from '../lib/automations';
import { revealInvisible } from '../lib/skills';

export interface AutomationsPageProps {
  view: AutomationsView;
  agentId: string;
  /** `LiveState.automationsVersion`, so an open history reads again. */
  version: number;
  online: boolean;
  /** The companion's Telegram connector with an approved chat, if any. */
  telegramConnectorId: string | null;
  discordDestinations?: { connectorId: string; label: string }[];
  /** An automation to open in the editor ("Edit automation" in a check-in). */
  focusId?: string | null;
  onFocusHandled?: () => void;
  /** Goes to the main chat, at the automation's card when it is loaded. */
  onShowInChat: ShowInChat;
}

const CHIP_LABELS: Record<
  Exclude<DestinationChip['kind'], 'discord'>,
  string
> = {
  main: 'Main chat',
  telegram: 'Telegram',
  note: '+ note in main chat',
  connector: 'Connected channel',
};

function chipLabel(chip: DestinationChip): string {
  return chip.kind === 'discord'
    ? `Discord ${revealInvisible(chip.label).text}`
    : CHIP_LABELS[chip.kind];
}

/** Spec §15.4: automations with their next run, last outcome, and
 *  failures; create and edit; Run now, pause and resume, delete; history.
 *  Everything an automation says lands in the main chat; the rows say where
 *  else it also goes. */
export function AutomationsPage({
  view,
  agentId,
  version,
  online,
  telegramConnectorId,
  discordDestinations = [],
  focusId = null,
  onFocusHandled,
  onShowInChat,
}: AutomationsPageProps) {
  const [editing, setEditing] = useState<{
    automation: Automation | null;
  } | null>(null);
  const [historyId, setHistoryId] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);

  useEffect(() => {
    if (!focusId) return;
    const focused = view.automations.find((item) => item.id === focusId);
    if (!focused) return;
    setEditing({ automation: focused });
    onFocusHandled?.();
  }, [focusId, view.automations, onFocusHandled]);

  const hasHeartbeat = view.automations.some(
    (item) => item.preset === 'heartbeat',
  );
  const history = historyId
    ? (view.automations.find((item) => item.id === historyId) ?? null)
    : null;

  const save = async (draft: AutomationDraft) => {
    const current = editing?.automation ?? null;
    let saved = false;
    if (current) {
      saved = await view.update(current, {
        // The daemon refuses an empty name; the target is sent only when
        // the owner changed it.
        ...(draft.name ? { name: draft.name } : {}),
        prompt: draft.prompt,
        ...(draft.target ? { target: draft.target } : {}),
        activeHours: draft.activeHours,
        ...(draft.trigger ? { trigger: draft.trigger } : {}),
      });
    } else if (draft.trigger) {
      saved = await view.create({
        prompt: draft.prompt,
        trigger: draft.trigger,
        target: draft.target ?? { type: 'workspace' },
        ...(draft.name ? { name: draft.name } : {}),
        ...(draft.activeHours ? { activeHours: draft.activeHours } : {}),
      });
    }
    if (saved) setEditing(null);
    return saved;
  };

  const groups = groupAutomations(view.automations);
  const chipContext = { telegramConnectorId, discordDestinations };

  const row = (
    automation: Automation,
    group: 'review' | 'upcoming' | 'repeating' | 'done',
  ) => {
    // Names and prompts can be the companion's: invisible
    // characters are shown as markers, never rendered.
    const name = revealInvisible(automation.name).text;
    const prompt = revealInvisible(automation.prompt).text;
    const interrupted = isInterrupted(automation);
    const finished = group === 'done';
    return (
      <li key={automation.id}>
        <article
          className="automation-row"
          aria-label={name}
          data-enabled={automation.enabled || undefined}
          data-done={finished || undefined}
        >
          <div className="automation-row-header">
            <h4 className="automation-name">{name}</h4>
            {automation.preset === 'heartbeat' && (
              <span className="automation-badge">Heartbeat</span>
            )}
            {automation.running && (
              <span className="automation-badge" data-running>
                Running
              </span>
            )}
            {!automation.enabled && !finished && (
              <span className="automation-badge">Paused</span>
            )}
            <ul className="automation-chips" aria-label="Goes to">
              {destinationChips(automation, chipContext).map((chip) => (
                <li
                  key={chip.kind}
                  className="automation-chip"
                  data-kind={chip.kind}
                >
                  {chipLabel(chip)}
                </li>
              ))}
            </ul>
          </div>
          <p className="automation-schedule">
            {describeTrigger(automation.trigger)}
            {automation.activeHours &&
              ` · ${describeActiveHours(automation.activeHours)}`}
          </p>
          <p className="automation-meta">
            {finished
              ? automation.doneAtMs != null
                ? `Done at ${formatWhen(automation.doneAtMs)}${automation.removesAtMs != null ? ` · clears ${formatWhen(automation.removesAtMs)}` : ''}`
                : automation.enabled
                  ? `Snoozed until ${formatWhen(automation.nextDueAtMs)}`
                  : `Waiting for you since ${formatWhen(automation.lastFiredAtMs ?? 0)}`
              : automation.enabled
                ? automation.trigger.type === 'event'
                  ? 'Waiting for event'
                  : `Next run ${formatWhen(automation.nextDueAtMs)}`
                : 'Paused'}
            {automation.lastOutcome &&
              ` · Last: ${interrupted ? 'Needs review' : OUTCOME_LABELS[automation.lastOutcome.status]}`}
            {automation.counters.consecutiveFailures > 0 &&
              ` · ${automation.counters.consecutiveFailures} failed in a row`}
          </p>
          {automation.createdBy.kind === 'agent' && (
            <p className="automation-note">{AGENT_CREATED_NOTE}</p>
          )}
          <pre className="automation-prompt">{prompt}</pre>
          {interrupted && (
            <p className="automation-note">
              <strong>Needs review</strong>. The interrupted run may have
              completed some effects. Check what it did in the main chat, and
              anything it touched, before using Run now or Resume.
            </p>
          )}
          <div className="automation-actions">
            {!finished && (
              <>
                <button
                  type="button"
                  className="studio-tool-button"
                  disabled={!online || automation.running}
                  onClick={() => void view.runNow(automation)}
                >
                  Run now
                </button>
                <button
                  type="button"
                  className="studio-tool-button"
                  disabled={!online}
                  onClick={() =>
                    void view.setEnabled(automation, !automation.enabled)
                  }
                >
                  {automation.enabled ? 'Pause' : 'Resume'}
                </button>
                <button
                  type="button"
                  className="studio-tool-button"
                  disabled={!online}
                  onClick={() => setEditing({ automation })}
                >
                  Edit
                </button>
              </>
            )}
            <button
              type="button"
              className="studio-tool-button"
              onClick={() => setHistoryId(automation.id)}
            >
              History
            </button>
            {interrupted && (
              <button
                type="button"
                className="studio-tool-button"
                onClick={() => setHistoryId(automation.id)}
              >
                Review interrupted run
              </button>
            )}
            {finished && (
              <button
                type="button"
                className="studio-tool-button"
                onClick={() => onShowInChat({ automationId: automation.id })}
              >
                Show in chat
              </button>
            )}
            {confirming === automation.id ? (
              <>
                <span className="automations-empty">
                  Delete it? Its cards stay in the main chat.
                </span>
                <button
                  type="button"
                  className="studio-tool-button"
                  onClick={() => {
                    setConfirming(null);
                    void view.remove(automation);
                  }}
                >
                  Confirm delete
                </button>
                <button
                  type="button"
                  className="studio-tool-button"
                  onClick={() => setConfirming(null)}
                >
                  Keep
                </button>
              </>
            ) : (
              <button
                type="button"
                className="studio-tool-button"
                disabled={!online}
                onClick={() => setConfirming(automation.id)}
              >
                Delete
              </button>
            )}
          </div>
        </article>
      </li>
    );
  };

  const section = (
    id: string,
    title: string,
    note: string | null,
    items: Automation[],
    kind: 'review' | 'upcoming' | 'repeating' | 'done',
  ) =>
    items.length > 0 && (
      <section
        className="automations-group"
        aria-labelledby={`automations-${id}`}
      >
        <h3 id={`automations-${id}`} className="automations-group-title">
          {title}
        </h3>
        {note && <p className="automations-group-note">{note}</p>}
        <ul className="automations-list">
          {items.map((automation) => row(automation, kind))}
        </ul>
      </section>
    );

  return (
    <div className="automations-page">
      {view.error && (
        <p className="automations-error" role="alert">
          {view.error}
        </p>
      )}
      <section
        className="automations-section"
        aria-labelledby="automations-heading"
      >
        <div className="automations-header">
          <h2 id="automations-heading" className="sr-only">
            Automations
          </h2>
          <button
            type="button"
            className="studio-tool-button"
            disabled={!online}
            onClick={() => setEditing({ automation: null })}
          >
            New automation
          </button>
          <button
            type="button"
            className="studio-tool-button"
            disabled={!online || hasHeartbeat}
            onClick={() => void view.createHeartbeat(localTimeZone())}
          >
            Add heartbeat
          </button>
        </div>
        {editing && (
          <AutomationEditor
            key={editing.automation?.id ?? 'new'}
            automation={editing.automation}
            telegramConnectorId={telegramConnectorId}
            discordDestinations={discordDestinations}
            onSave={save}
            onCancel={() => setEditing(null)}
          />
        )}
        {view.loaded && view.automations.length === 0 ? (
          <p className="automations-empty">
            No automations yet. Create one, add the heartbeat, or ask your
            companion to schedule something.
          </p>
        ) : (
          <>
            {section(
              'review',
              'Needs review',
              null,
              groups.needsReview,
              'review',
            )}
            {section(
              'upcoming',
              'Upcoming',
              'One-time. Each one clears itself a day after it’s done.',
              groups.upcoming,
              'upcoming',
            )}
            {section(
              'repeating',
              'Repeating',
              null,
              groups.repeating,
              'repeating',
            )}
            {section(
              'done',
              'Done',
              'Kept for a day so you can find them, then removed. The card in the chat stays.',
              groups.done,
              'done',
            )}
          </>
        )}
      </section>
      {history && (
        <AutomationHistory
          agentId={agentId}
          automation={history}
          version={version}
          online={online}
          onClose={() => setHistoryId(null)}
          onShowInChat={onShowInChat}
        />
      )}
    </div>
  );
}

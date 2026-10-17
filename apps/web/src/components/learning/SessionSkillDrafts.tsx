import { useEffect, useState } from 'react';
import type { SkillDraft } from '@animaOS-SWARM/sdk';

import { daemon } from '../../lib/daemon-api';
import { revealInvisible } from '../../lib/skills';

type Outcome = 'kept' | 'discarded';

/** Skill drafts anima proposed in this conversation, offered where they were
 *  learned (spec §15.4). The Skills page keeps the full review and editor. */
export function SessionSkillDrafts({
  sessionId,
  version,
  epoch,
  enabled,
  onReview,
}: {
  sessionId: string;
  /** `LiveState.skillsVersion`: drafts are read again when it moves. */
  version: number;
  epoch: number;
  enabled: boolean;
  onReview?: () => void;
}) {
  const [drafts, setDrafts] = useState<SkillDraft[]>([]);
  const [hidden, setHidden] = useState<ReadonlySet<string>>(new Set());
  const [busy, setBusy] = useState<string | null>(null);
  const [outcomes, setOutcomes] = useState<Record<string, Outcome>>({});
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!enabled) return;
    const controller = new AbortController();
    daemon
      .listSkillDrafts({ status: 'pending', signal: controller.signal })
      .then(
        (pending) => {
          if (controller.signal.aborted) return;
          setDrafts(
            pending.filter(
              (draft) =>
                draft.proposedBy?.sessionId === sessionId &&
                draft.problem === null,
            ),
          );
        },
        // The Skills page reports read errors; here a draft is optional.
        () => undefined,
      );
    return () => controller.abort();
  }, [sessionId, version, epoch, enabled]);

  const decide = async (draft: SkillDraft, outcome: Outcome) => {
    if (busy) return;
    setBusy(draft.id);
    setError(null);
    try {
      if (outcome === 'kept')
        await daemon.approveSkillDraft(
          draft.id,
          draft.source === 'file' && draft.fileHash
            ? { hash: draft.fileHash }
            : {},
        );
      else await daemon.rejectSkillDraft(draft.id);
      setOutcomes((current) => ({ ...current, [draft.id]: outcome }));
    } catch (caught) {
      setError(
        caught instanceof Error ? caught.message : 'The skill was not saved.',
      );
    } finally {
      setBusy(null);
    }
  };

  const shown = drafts.filter(
    (draft) => !hidden.has(draft.id) || outcomes[draft.id],
  );
  if (shown.length === 0) return null;

  return (
    <div className="session-skill-drafts">
      {shown.map((draft) => {
        const outcome = outcomes[draft.id];
        return (
          <section
            key={draft.id}
            className="learned-skill"
            aria-label={`New skill: ${draft.name}`}
          >
            <p className="learned-skill-kicker">
              New skill · learned in this chat
            </p>
            <h3>{revealInvisible(draft.name).text}</h3>
            {draft.description && (
              <p className="learned-skill-desc">
                {revealInvisible(draft.description).text}
              </p>
            )}
            {outcome ? (
              <p role="status" className="learned-skill-outcome">
                {outcome === 'kept'
                  ? 'Kept. anima will use it next time.'
                  : 'Discarded.'}
              </p>
            ) : (
              <div className="learned-skill-actions">
                <button
                  type="button"
                  className="learned-skill-keep"
                  disabled={busy !== null}
                  onClick={() => void decide(draft, 'kept')}
                >
                  {busy === draft.id ? 'Saving…' : 'Keep skill'}
                </button>
                {onReview && (
                  <button
                    type="button"
                    disabled={busy !== null}
                    onClick={onReview}
                  >
                    Review in Skills
                  </button>
                )}
                <button
                  type="button"
                  disabled={busy !== null}
                  onClick={() =>
                    setHidden((current) => new Set(current).add(draft.id))
                  }
                >
                  Not now
                </button>
                <button
                  type="button"
                  className="learned-skill-discard"
                  disabled={busy !== null}
                  onClick={() => void decide(draft, 'discarded')}
                >
                  Discard
                </button>
              </div>
            )}
          </section>
        );
      })}
      {error && (
        <p role="alert" className="learned-skill-error">
          {error}
        </p>
      )}
    </div>
  );
}

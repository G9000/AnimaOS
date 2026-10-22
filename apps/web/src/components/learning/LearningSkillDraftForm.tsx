import { useEffect, useRef, useState, type ReactNode } from 'react';
import type {
  LearningProposal,
  LearningReview,
  Skill,
  SkillDraft,
  SkillDraftStatus,
} from '@animaos/sdk';
import { DaemonHttpError } from '@animaos/sdk';
import type { LearningReviewApi } from './LearningReviewPanel';

interface SkillFields {
  slug: string;
  name: string;
  description: string;
  body: string;
  expectedHash: string | null;
}

export function LearningSkillDraftForm({
  agentId,
  review,
  proposal,
  api,
  disabled,
  onCreated,
  onSavingChange,
  onOpenSkills,
  children,
}: {
  agentId: string;
  review: LearningReview;
  proposal: LearningProposal;
  api?: LearningReviewApi;
  disabled: boolean;
  onCreated: (draft: SkillDraft) => void;
  onSavingChange?: (saving: boolean) => boolean;
  onOpenSkills?: () => void;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const [skills, setSkills] = useState<Skill[]>([]);
  const [fields, setFields] = useState(() => initialFields(proposal));
  const [loading, setLoading] = useState(false);
  const [loadingSkill, setLoadingSkill] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [draftId, setDraftId] = useState(proposal.skillDraftId ?? null);
  const [createdStatus, setCreatedStatus] = useState<SkillDraftStatus | null>(
    null,
  );
  const [selectedSkill, setSelectedSkill] = useState<Skill | null>(null);
  const selection = useRef(0);
  const lifetime = useRef(0);

  useEffect(() => {
    const current = ++lifetime.current;
    return () => {
      if (lifetime.current === current) lifetime.current += 1;
      selection.current += 1;
    };
  }, [agentId, review.id, proposal.id]);

  useEffect(() => {
    if (proposal.skillDraftId) setDraftId(proposal.skillDraftId);
  }, [proposal.skillDraftId]);

  useEffect(() => {
    if (!open || !api) return;
    const controller = new AbortController();
    setLoading(true);
    void api
      .listSkills({ signal: controller.signal })
      .then(
        (next) => {
          if (!controller.signal.aborted) setSkills(next);
        },
        (cause: unknown) => {
          if (!controller.signal.aborted) setError(errorMessage(cause));
        },
      )
      .finally(() => {
        if (!controller.signal.aborted) setLoading(false);
      });
    return () => controller.abort();
  }, [api, open]);

  const chooseSkill = async (slug: string) => {
    const request = ++selection.current;
    setError(null);
    if (!slug) {
      setSelectedSkill(null);
      setFields(initialFields(proposal));
      setLoadingSkill(false);
      return;
    }
    const skill = skills.find((item) => item.slug === slug);
    if (!api || !skill) return;
    setSelectedSkill(null);
    setFields(initialFields(proposal));
    setLoadingSkill(true);
    try {
      const detail = await api.skill(slug);
      if (request !== selection.current) return;
      if (!detail.skill || detail.file?.hash == null) {
        setSelectedSkill(null);
        setError(
          'The current skill file could not be read. Refresh Skills and review it before creating a draft.',
        );
        return;
      }
      setSelectedSkill(detail.skill);
      const existingBody = detail.file.body?.trimEnd() ?? '';
      setFields({
        slug: detail.skill.slug,
        name: detail.skill.name,
        description: detail.skill.description,
        body: existingBody
          ? `${existingBody}\n\n${proposal.suggestion}`
          : proposal.suggestion,
        expectedHash: detail.file.hash,
      });
    } catch (cause) {
      if (request === selection.current) setError(errorMessage(cause));
    } finally {
      if (request === selection.current) setLoadingSkill(false);
    }
  };

  const create = async () => {
    if (!api || saving || disabled || loading || loadingSkill) return;
    if (onSavingChange && !onSavingChange(true)) return;
    const requestLifetime = lifetime.current;
    setSaving(true);
    setError(null);
    try {
      const draft = await api.createSkillDraft(
        agentId,
        review.id,
        proposal.id,
        {
          expectedVersion: proposal.decisionVersion,
          evidenceDigest: review.evidenceDigest,
          skill: {
            slug: fields.slug,
            name: fields.name,
            description: fields.description,
            body: fields.body,
            expectedHash: fields.expectedHash,
          },
        },
      );
      if (lifetime.current === requestLifetime) {
        setDraftId(draft.id);
        setCreatedStatus(draft.status);
        setOpen(false);
        onCreated(draft);
      }
    } catch (cause) {
      if (lifetime.current === requestLifetime)
        setError(
          cause instanceof DaemonHttpError && cause.status === 409
            ? 'The review evidence or selected skill changed. Your edits are kept; read the review again before creating another draft.'
            : errorMessage(cause),
        );
    } finally {
      if (lifetime.current === requestLifetime) setSaving(false);
      onSavingChange?.(false);
    }
  };

  const update = (key: keyof SkillFields, value: string) => {
    setFields((current) => ({
      ...current,
      [key]: value,
      ...(key === 'slug' && selectedSkill?.slug !== value
        ? { expectedHash: null }
        : {}),
    }));
    if (key === 'slug' && selectedSkill?.slug !== value) setSelectedSkill(null);
  };

  return (
    <section className="rounded-lg border border-line bg-black/10 p-3">
      {children}
      {draftId ? (
        <div className="mt-3 rounded-lg border border-line bg-white/[0.02] p-3">
          <p className="text-sm text-ink">
            {createdStatus === 'pending'
              ? 'Pending skill draft created. It still needs your separate review and approval.'
              : createdStatus
                ? `Linked skill draft status: ${createdStatus}. Open Skills to review it.`
                : 'A skill draft is linked to this advice. Open Skills to check its current status.'}
          </p>
          {onOpenSkills ? (
            <button
              type="button"
              className="studio-tool-button mt-2"
              onClick={onOpenSkills}
            >
              Open skill draft in Skills
            </button>
          ) : null}
        </div>
      ) : proposal.decision !== 'dismissed' && api ? (
        <div className="mt-3">
          {!open ? (
            <button
              type="button"
              className="studio-tool-button"
              disabled={disabled}
              onClick={() => setOpen(true)}
            >
              Create skill draft
            </button>
          ) : (
            <form
              aria-label="Create skill draft from learning advice"
              className="space-y-3 rounded-lg border border-line bg-white/[0.02] p-3"
              onSubmit={(event) => {
                event.preventDefault();
                void create();
              }}
            >
              <p className="text-sm text-ink-2">
                This creates a pending draft only. The skill will not be saved
                or enabled until you review and approve it in Skills.
              </p>
              <label className="flex flex-col gap-1 text-sm text-ink">
                Base skill
                <select
                  value={selectedSkill?.slug ?? ''}
                  disabled={disabled || loading || saving}
                  onChange={(event) => void chooseSkill(event.target.value)}
                >
                  <option value="">New skill</option>
                  {skills.map((skill) => (
                    <option key={skill.slug} value={skill.slug}>
                      {skill.name} (/{skill.slug})
                    </option>
                  ))}
                </select>
              </label>
              {loading ? <p role="status">Loading skills…</p> : null}
              {loadingSkill ? (
                <p role="status">Loading selected skill…</p>
              ) : null}
              <label className="flex flex-col gap-1 text-sm text-ink">
                Skill slug
                <input
                  required
                  value={fields.slug}
                  disabled={disabled || saving || loadingSkill}
                  onChange={(event) => update('slug', event.target.value)}
                />
              </label>
              <label className="flex flex-col gap-1 text-sm text-ink">
                Skill name
                <input
                  required
                  value={fields.name}
                  disabled={disabled || saving || loadingSkill}
                  onChange={(event) => update('name', event.target.value)}
                />
              </label>
              <label className="flex flex-col gap-1 text-sm text-ink">
                Skill description
                <input
                  required
                  value={fields.description}
                  disabled={disabled || saving || loadingSkill}
                  onChange={(event) =>
                    update('description', event.target.value)
                  }
                />
              </label>
              <label className="flex flex-col gap-1 text-sm text-ink">
                Skill instructions
                <textarea
                  required
                  rows={8}
                  value={fields.body}
                  disabled={disabled || saving || loadingSkill}
                  onChange={(event) => update('body', event.target.value)}
                />
              </label>
              {fields.expectedHash ? (
                <p className="text-xs text-ink-3">
                  Based on the selected skill version{' '}
                  {fields.expectedHash.slice(0, 12)}.
                </p>
              ) : null}
              {error ? (
                <p role="alert" className="text-sm text-danger">
                  {error}
                </p>
              ) : null}
              <div className="flex gap-2">
                <button
                  type="submit"
                  className="studio-tool-button"
                  disabled={
                    disabled ||
                    saving ||
                    loading ||
                    loadingSkill ||
                    !fields.slug.trim() ||
                    !fields.name.trim() ||
                    !fields.description.trim() ||
                    !fields.body.trim()
                  }
                >
                  {saving ? 'Creating draft…' : 'Create pending skill draft'}
                </button>
                <button
                  type="button"
                  className="studio-tool-button"
                  disabled={saving}
                  onClick={() => setOpen(false)}
                >
                  Cancel
                </button>
              </div>
            </form>
          )}
        </div>
      ) : null}
    </section>
  );
}

function initialFields(proposal: LearningProposal): SkillFields {
  const name =
    proposal.observation.trim().replace(/\s+/g, ' ').slice(0, 64) ||
    'Reviewed guidance';
  const slug =
    name
      .normalize('NFKD')
      .replace(/[\u0300-\u036f]/g, '')
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-|-$/g, '')
      .slice(0, 64) || 'reviewed-guidance';
  return {
    slug,
    name,
    description: proposal.observation.slice(0, 300),
    body: proposal.suggestion,
    expectedHash: null,
  };
}

function errorMessage(cause: unknown): string {
  return cause instanceof Error
    ? cause.message
    : 'Skill draft creation failed.';
}

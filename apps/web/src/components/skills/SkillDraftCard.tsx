import { useResetOnChange } from '../../hooks/useResetOnChange';
import { useEffect, useMemo, useState } from 'react';
import type {
  SkillDraft,
  SkillDraftApproval,
  SkillDraftProposer,
} from '@animaos/sdk';

import { daemon } from '../../lib/daemon-api';
import { lineDiff, type DiffLine } from '../../lib/skill-diff';
import {
  SOURCE_LABELS,
  hasControlCharacter,
  invisibleNote,
  revealInvisible,
} from '../../lib/skills';
import { SkillEditor } from './SkillEditor';
import { SkillSupportingFiles } from './SkillSupportingFiles';

const DIFF_PREFIX: Record<DiffLine['kind'], string> = {
  same: '  ',
  added: '+ ',
  removed: '- ',
};

/** Source metadata is untrusted. Never expose credentials or URL parameters. */
function safeSourceUrl(value: string): string | null {
  if (
    !/^https:\/\//i.test(value) ||
    /[\s\\]/u.test(value) ||
    hasControlCharacter(value)
  ) return null;
  try {
    const url = new URL(value);
    if (
      url.protocol !== 'https:' ||
      url.username ||
      url.password ||
      url.search ||
      url.hash ||
      value.slice(8).split(/[/?#]/u)[0].includes('@')
    ) return null;
    return url.href;
  } catch {
    return null;
  }
}

function SourceUrl({ value }: { value: string }) {
  const url = safeSourceUrl(value);
  return url ? (
    <a href={url} target="_blank" rel="noopener noreferrer">{url}</a>
  ) : (
    <span>Source URL unavailable for safe linking.</span>
  );
}

/** Reveal BOM/control formatting without interpreting the original as markup. */
function originalInstructions(text: string): string {
  return revealInvisible(text).text
    .replace(/\r/g, '⟨CR⟩')
    .replace(/\n/g, '⟨LF⟩\n');
}

/** A draft waiting for the owner (spec §15.4). Its text was written by the
 *  model, an import, or a file: it is shown only as text. */
export function SkillDraftCard({
  draft,
  onApprove,
  onReject,
  onOpenSession,
}: {
  draft: SkillDraft;
  onApprove: (
    draft: SkillDraft,
    approval: SkillDraftApproval,
  ) => Promise<boolean>;
  onReject: (draft: SkillDraft) => Promise<boolean>;
  onOpenSession?: (proposer: SkillDraftProposer) => void;
}) {
  const [current, setCurrent] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [busy, setBusy] = useState(false);

  // A draft that replaces a skill is compared with that skill's file now.
  useResetOnChange([draft.slug, draft.currentHash], () => setCurrent(null));
  useEffect(() => {
    if (draft.currentHash === null) return;
    const controller = new AbortController();
    daemon.skill(draft.slug, { signal: controller.signal }).then(
      (detail) => {
        if (!controller.signal.aborted) setCurrent(detail.file?.body ?? null);
      },
      () => undefined,
    );
    return () => controller.abort();
  }, [draft.slug, draft.currentHash]);

  const approve = async (approval: SkillDraftApproval) => {
    setBusy(true);
    const kept = await onApprove(
      draft,
      draft.source === 'file' && draft.fileHash
        ? { ...approval, hash: draft.fileHash }
        : approval,
    );
    setBusy(false);
    if (kept) setEditing(false);
    return kept;
  };
  const reject = async () => {
    setBusy(true);
    await onReject(draft);
    setBusy(false);
  };
  const diff = useMemo(
    () => (current === null ? null : lineDiff(current, draft.body)),
    [current, draft.body],
  );
  const blocked = busy || draft.problem !== null;
  // What the owner reads is shown with the invisible characters as markers;
  // the diff is computed on the raw text and each line is revealed.
  const name = revealInvisible(draft.name);
  const description = revealInvisible(draft.description);
  const body = revealInvisible(draft.body);
  const hidden =
    name.count +
    description.count +
    body.count +
    (diff ? revealInvisible(current ?? '').count : 0);
  const note = invisibleNote(hidden);
  const learningSource = draft.learningSource;
  const githubSource = draft.githubSource;
  const urlSource = draft.urlSource;
  const observed = urlSource ? new Date(urlSource.observedAtMs) : null;
  const observedIso = observed && Number.isFinite(observed.getTime())
    ? observed.toISOString()
    : null;
  const originalSkillText = urlSource?.originalSkillText ?? githubSource?.originalSkillText;
  const safeGithubUrl = githubSource ? safeSourceUrl(githubSource.sourceUrl) : null;
  const githubUrl = safeGithubUrl?.startsWith('https://github.com/')
    ? safeGithubUrl
    : null;
  const sourceChat = learningSource
    ? {
        agentId: learningSource.agentId,
        sessionId: learningSource.sessionId,
        runId: learningSource.sourceRunId,
      }
    : draft.proposedBy;

  return (
    <section className="skill-draft" aria-label={`Skill draft: ${name.text}`}>
      <p className="skill-draft-kicker">New skill to keep</p>
      <header className="skill-draft-header">
        <strong>{name.text}</strong>
        <code>/{draft.slug}</code>
        <span className="skill-draft-source">
          {SOURCE_LABELS[draft.source]}
        </span>
        {sourceChat && onOpenSession && (
          <button
            type="button"
            className="studio-tool-button"
            onClick={() => onOpenSession(sourceChat)}
          >
            {learningSource ? 'Open source task' : 'Open the chat'}
          </button>
        )}
      </header>
      {githubSource && (
        <p className="skill-draft-warning" role="note">
          Imported from{' '}
          {githubUrl ? (
            <a href={githubUrl} target="_blank" rel="noopener noreferrer">
              {githubSource.repository}
              {githubSource.path ? `/${githubSource.path}` : ''}
            </a>
          ) : (
            githubSource.repository
          )}{' '}
          at commit <code>{githubSource.commit.slice(0, 12)}</code>. Imported
          files remain inactive until you keep this skill.
          {githubSource.ignoredPaths.length > 0 && (
            <> Repository metadata excluded: {githubSource.ignoredPaths.join(', ')}.</>
          )}
        </p>
      )}
      {urlSource && (
        <>
          <p className="skill-draft-warning" role="note">
            Captured from <SourceUrl value={urlSource.sourceUrl} />. This
            capture includes referenced or selected files and does not
            represent a complete repository or an immutable commit. Imported
            files remain inactive until you keep this skill.
            {urlSource.requestedUrl !== urlSource.sourceUrl && (
              <> Requested URL: <SourceUrl value={urlSource.requestedUrl} />;
                redirected to a different source URL shown above.</>
            )}
          </p>
          <details>
            <summary>Captured source details</summary>
            <p>
              Observed:{' '}
              {observedIso ? (
                <time dateTime={observedIso}>{observedIso}</time>
              ) : 'Observation time unavailable.'}
            </p>
            <p>Captured package SHA-256: <code>{urlSource.packageHash}</code></p>
            <ul aria-label="Captured source files">
              {urlSource.files.map((file) => (
                <li key={file.path}>
                  <code>{revealInvisible(file.path).text}</code>{' '}
                  (<span>{file.sizeBytes.toLocaleString()} bytes</span>)
                  <p>Source: <SourceUrl value={file.sourceUrl} /></p>
                  <p>SHA-256: <code>{file.sha256}</code></p>
                </li>
              ))}
            </ul>
          </details>
        </>
      )}
      {learningSource && (
        <p className="skill-draft-warning" role="note">
          Created from unverified learning advice in review{' '}
          {learningSource.reviewId}, source run {learningSource.sourceRunId},
          evidence {learningSource.evidenceDigest.slice(0, 12)}. Review and
          approve this draft separately.
        </p>
      )}
      {originalSkillText != null && (
        <details>
          <summary>Original package instructions and metadata</summary>
          <p>Formatting markers: ⟨CR⟩ is a carriage return; ⟨LF⟩ is a line feed.</p>
          <pre className="skill-body" aria-label="Original package instructions">{originalInstructions(originalSkillText)}</pre>
        </details>
      )}
      {description.text && (
        <p className="skill-draft-description">{description.text}</p>
      )}
      {note && (
        <p className="skill-draft-warning" role="note">
          {note}
        </p>
      )}
      {draft.stale && (
        <p className="skill-draft-warning" role="note">
          This skill changed since the draft was made; the comparison is with
          its current version.
        </p>
      )}
      {draft.problem && (
        <p className="skill-draft-warning" role="note">
          {draft.problem}
        </p>
      )}
      {editing ? (
        <SkillEditor
          bodyOnly
          initial={{
            slug: draft.slug,
            name: draft.name,
            description: draft.description,
            body: draft.body,
            files: draft.files,
          }}
          saveLabel="Approve edited version"
          onSave={(value) =>
            approve({
              body: value.body,
              ...(value.files !== undefined ? { files: value.files } : {}),
            })
          }
          onCancel={() => setEditing(false)}
        />
      ) : (
        <>
            <div className="skill-draft-actions">
              <details className="skill-draft-steps">
                <summary>See steps</summary>
                {diff ? (
                  <pre
                    className="skill-diff"
                    aria-label="Changes from the current version"
                  >
                    {diff.map((line, index) => (
                      <span key={index} className={`skill-diff-${line.kind}`}>
                        {DIFF_PREFIX[line.kind]}
                        {revealInvisible(line.text).text}
                        {'\n'}
                      </span>
                    ))}
                  </pre>
                ) : (
                  <pre className="skill-body" aria-label="Instructions">
                    {body.text}
                  </pre>
                )}
                <SkillSupportingFiles files={draft.files} />
              </details>
              <div className="skill-actions">
                <button
                  type="button"
                  className="studio-tool-button"
                  disabled={blocked}
                  onClick={() => setEditing(true)}
                >
                  Edit
                </button>
                <button
                  type="button"
                  className="studio-tool-button skill-draft-discard"
                  disabled={busy}
                  onClick={() => void reject()}
                >
                  Discard
                </button>
                <button
                  type="button"
                  className="studio-tool-button skill-draft-keep"
                  disabled={blocked}
                  onClick={() => void approve({})}
                >
                  Keep
                </button>
              </div>
            </div>
        </>
      )}
    </section>
  );
}

import { useState, type FormEvent } from 'react';
import type { SkillSupportingFile } from '@animaOS-SWARM/sdk';
import { SkillSupportingFiles } from './SkillSupportingFiles';

import {
  invisibleNote,
  revealInvisible,
  skillInputProblem,
  slugFromName,
} from '../../lib/skills';

export interface SkillEditorValue {
  slug: string;
  name: string;
  description: string;
  body: string;
  files?: SkillSupportingFile[];
}

/** Writes or edits a skill's content (spec §15.4). The preview shows the
 *  body as plain text: drafts may hold model-written text, so nothing
 *  here is ever rendered as Markdown or HTML. */
export function SkillEditor({
  initial,
  slugLocked = false,
  bodyOnly = false,
  saveLabel,
  checkProblem,
  onSave,
  onCancel,
}: {
  initial?: Partial<SkillEditorValue>;
  /** An existing skill keeps its folder. */
  slugLocked?: boolean;
  /** A draft's approval may change only its body (spec §8.4); `initial`
   *  still carries the draft's slug, name, and description, which the
   *  check before saving reads. */
  bodyOnly?: boolean;
  saveLabel: string;
  /** Anything else the caller would refuse in this content, or null. */
  checkProblem?: (value: SkillEditorValue) => string | null;
  /** True when the daemon took it; the editor then stays for the caller
   *  to close. */
  onSave: (value: SkillEditorValue) => Promise<boolean>;
  onCancel: () => void;
}) {
  const [name, setName] = useState(initial?.name ?? '');
  const [description, setDescription] = useState(initial?.description ?? '');
  const [body, setBody] = useState(initial?.body ?? '');
  const [files, setFiles] = useState<SkillSupportingFile[]>(
    initial?.files ?? [],
  );
  const [filesEdited, setFilesEdited] = useState(initial?.files !== undefined);
  const [slug, setSlug] = useState(initial?.slug ?? '');
  const [slugEdited, setSlugEdited] = useState(Boolean(initial?.slug));
  const [preview, setPreview] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const folder = slugEdited ? slug : slugFromName(name);
  const shownBody = revealInvisible(body);
  const bodyNote = invisibleNote(shownBody.count);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const value = {
      slug: folder,
      name,
      description,
      body,
      ...(filesEdited ? { files } : {}),
    };
    const fileProblem = supportingFilesProblem(body, files);
    const found =
      skillInputProblem(value) ?? fileProblem ?? checkProblem?.(value) ?? null;
    setProblem(found);
    if (found) return;
    setSaving(true);
    await onSave(value);
    setSaving(false);
  };

  return (
    <form
      className="skill-editor"
      aria-label="Skill editor"
      onSubmit={(event) => void submit(event)}
    >
      {!bodyOnly && (
        <>
          <label className="skill-field">
            <span>Name</span>
            <input
              value={name}
              onChange={(event) => setName(event.target.value)}
            />
          </label>
          <label className="skill-field">
            <span>Folder name</span>
            <input
              value={folder}
              disabled={slugLocked}
              onChange={(event) => {
                setSlug(event.target.value);
                setSlugEdited(true);
              }}
            />
          </label>
          <label className="skill-field">
            <span>When to use it</span>
            <input
              value={description}
              onChange={(event) => setDescription(event.target.value)}
            />
          </label>
        </>
      )}
      {preview ? (
        <pre className="skill-body" aria-label="Preview">
          {shownBody.text}
        </pre>
      ) : (
        <label className="skill-field">
          <span>Instructions</span>
          <textarea
            rows={12}
            value={body}
            onChange={(event) => setBody(event.target.value)}
          />
        </label>
      )}
      {bodyNote && (
        <p className="skill-draft-warning" role="note">
          {bodyNote}
        </p>
      )}
      {preview ? (
        <SkillSupportingFiles files={files} />
      ) : (
        <section aria-label="Supporting file editor">
          <p>
            Supporting files are reviewed with these instructions. Scripts stay
            inert until separately run.
          </p>
          {files.map((file, index) => (
            <div key={index}>
              <label className="skill-field">
                <span>File path {index + 1}</span>
                <input
                  value={file.path}
                  placeholder="templates/note.md"
                  onChange={(event) => {
                    setFilesEdited(true);
                    setFiles(
                      files.map((item, i) =>
                        i === index
                          ? { ...item, path: event.target.value }
                          : item,
                      ),
                    );
                  }}
                />
              </label>
              <label className="skill-field">
                <span>File content {index + 1}</span>
                <textarea
                  rows={6}
                  value={file.content}
                  onChange={(event) => {
                    setFilesEdited(true);
                    setFiles(
                      files.map((item, i) =>
                        i === index
                          ? { ...item, content: event.target.value }
                          : item,
                      ),
                    );
                  }}
                />
              </label>
              <button
                type="button"
                className="studio-tool-button"
                onClick={() => {
                  setFilesEdited(true);
                  setFiles(files.filter((_, i) => i !== index));
                }}
              >
                Remove file {index + 1}
              </button>
            </div>
          ))}
          <button
            type="button"
            className="studio-tool-button"
            disabled={files.length >= 32}
            onClick={() => {
              setFilesEdited(true);
              setFiles([...files, { path: '', content: '' }]);
            }}
          >
            Add supporting file
          </button>
        </section>
      )}
      {problem && (
        <p className="skills-error" role="alert">
          {problem}
        </p>
      )}
      <div className="skill-actions">
        <button
          type="button"
          className="studio-tool-button"
          onClick={() => setPreview((value) => !value)}
        >
          {preview ? 'Edit text' : 'Preview'}
        </button>
        <button type="submit" className="studio-tool-button" disabled={saving}>
          {saveLabel}
        </button>
        <button type="button" className="studio-tool-button" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </form>
  );
}

function supportingFilesProblem(
  body: string,
  files: SkillSupportingFile[],
): string | null {
  const seen = new Set<string>();
  let total = new TextEncoder().encode(body).length + 4096;
  for (const file of files) {
    const parts = file.path.split('/');
    if (
      parts.length < 2 ||
      parts.length > 8 ||
      file.path.length > 240 ||
      !['references', 'templates', 'scripts', 'examples', 'assets'].includes(
        parts[0],
      ) ||
      parts.some(
        (part) =>
          !part ||
          part.startsWith('.') ||
          /[. ]$/.test(part) ||
          !/^[a-zA-Z0-9_.-]+$/.test(part) ||
          /^(con|prn|aux|nul|com[0-9]|lpt[0-9])(\.|$)/i.test(part),
      )
    )
      return 'Use a relative text file path such as templates/note.md.';
    if (seen.has(file.path.toLowerCase()))
      return 'Each supporting file needs a unique path.';
    seen.add(file.path.toLowerCase());
    const bytes = new TextEncoder().encode(file.content).length;
    if (bytes > 65536) return 'A supporting file must be at most 64 KiB.';
    total += bytes;
  }
  return files.length > 32
    ? 'A package supports at most 32 supporting files.'
    : total > 524288
      ? 'A complete skill package must be at most 512 KiB.'
      : null;
}

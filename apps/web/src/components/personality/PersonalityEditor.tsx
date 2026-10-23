import { useEffect, useId, useRef, useState } from 'react';
import {
  DaemonHttpError,
  type AgentPersonality,
  type SaveAgentPersonality,
} from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';
import {
  PERSONALITY_SECTIONS,
  personalitySection,
  updatePersonalitySection,
  type PersonalitySectionId,
} from '../../lib/personality-document';
import './personality.css';
import { useResetOnChange } from '../../hooks/useResetOnChange';

type Loader = (
  id: string,
  options: { signal?: AbortSignal },
) => Promise<AgentPersonality>;
type Saver = (
  id: string,
  input: SaveAgentPersonality,
) => Promise<AgentPersonality>;
const SOURCE_LABELS = {
  agent_config: 'Saved agent personality',
  workspace_soul: 'Workspace personality',
  profile_soul: 'Agent personality file',
};
type GuidedFields = Record<PersonalitySectionId, string>;
const guidedFields = (document: string): GuidedFields =>
  Object.fromEntries(
    PERSONALITY_SECTIONS.map((section) => [
      section.id,
      personalitySection(document, section.id),
    ]),
  ) as GuidedFields;

export function PersonalityEditor({
  agentId,
  disabled = false,
  loadPersonality = daemon.agentPersonality,
  savePersonality = daemon.saveAgentPersonality,
}: {
  agentId: string;
  disabled?: boolean;
  loadPersonality?: Loader;
  savePersonality?: Saver;
}) {
  const id = useId();
  const [saved, setSaved] = useState<AgentPersonality | null>(null);
  const [draft, setDraft] = useState('');
  const [fields, setFields] = useState<GuidedFields>(() => guidedFields(''));
  const guideBase = useRef('');
  const [view, setView] = useState<'guided' | 'markdown'>('guided');
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [conflict, setConflict] = useState(false);
  const [notice, setNotice] = useState('');
  const [attempt, setAttempt] = useState(0);
  const generation = useRef({ value: 0 });
  const savePending = useRef(false);

  useResetOnChange([agentId, attempt, loadPersonality], () => {
    setLoading(true);
    setSaved(null);
    setDraft('');
    setSaving(false);
    setConflict(false);
    setError(null);
    setNotice('');
  });
  useEffect(() => {
    const counter = generation.current;
    const current = ++counter.value;
    const controller = new AbortController();
    savePending.current = false;
    void loadPersonality(agentId, { signal: controller.signal }).then(
      (result) => {
        if (counter.value !== current) return;
        setSaved(result);
        setDraft(result.content);
        guideBase.current = result.content;
        setFields(guidedFields(result.content));
        setLoading(false);
      },
      (caught) => {
        if (counter.value !== current) return;
        setError(
          caught instanceof Error
            ? caught.message
            : 'Could not load personality.',
        );
        setLoading(false);
      },
    );
    return () => {
      ++counter.value;
      controller.abort();
    };
  }, [agentId, attempt, loadPersonality]);

  const used = [...draft].length;
  const dirty = saved !== null && draft !== saved.content;
  const valid = draft.trim().length > 0 && used <= (saved?.maxChars ?? 0);
  const busy = disabled || loading || saving;
  const save = async () => {
    if (!saved || !dirty || !valid || busy || conflict || savePending.current)
      return;
    const counter = generation.current;
    const current = counter.value;
    savePending.current = true;
    setSaving(true);
    setError(null);
    setNotice('');
    try {
      const result = await savePersonality(agentId, {
        content: draft,
        revision: saved.revision,
      });
      if (counter.value !== current) return;
      setSaved(result);
      setDraft(result.content);
      guideBase.current = result.content;
      setFields(guidedFields(result.content));
      setNotice('Personality saved. New turns will use these instructions.');
    } catch (caught) {
      if (counter.value !== current) return;
      setConflict(caught instanceof DaemonHttpError && caught.status === 409);
      setError(
        caught instanceof Error
          ? caught.message
          : 'Could not save personality.',
      );
    } finally {
      if (counter.value === current) {
        savePending.current = false;
        setSaving(false);
      }
    }
  };

  const changeField = (sectionId: PersonalitySectionId, value: string) => {
    const next = { ...fields, [sectionId]: value };
    setFields(next);
    // Reapply edits to the source template so formatting separators cannot grow
    // on every keystroke. Field state retains trailing spaces and Enter presses.
    let document = guideBase.current;
    for (const section of PERSONALITY_SECTIONS) {
      if (
        next[section.id] !== personalitySection(guideBase.current, section.id)
      ) {
        document = updatePersonalitySection(
          document,
          section.id,
          next[section.id],
        );
      }
    }
    setDraft(document);
    setNotice('');
  };

  return (
    <section
      className="settings-group personality-editor"
      aria-labelledby={`${id}-title`}
      aria-busy={loading || saving}
    >
      <div className="personality-heading">
        <h3 id={`${id}-title`} className="settings-group-title">
          Personality
        </h3>
        {saved && (
          <span className="personality-source">
            {SOURCE_LABELS[saved.source]}
          </span>
        )}
      </div>
      <p className="settings-group-help">
        Give your companion a point of view, a purpose, and a voice. Changes
        apply to turns started after saving; a turn already in progress keeps
        its current personality.
        {saved?.source === 'workspace_soul' &&
          ' This personality is shared by agents using this workspace file.'}
      </p>
      {loading && <p role="status">Loading personality…</p>}
      {error && (
        <p role="alert" className="personality-error">
          {error}
        </p>
      )}
      {!loading && !saved && (
        <button
          type="button"
          className="personality-secondary"
          disabled={disabled}
          onClick={() => setAttempt((value) => value + 1)}
        >
          Retry loading personality
        </button>
      )}
      {saved && (
        <>
          <div
            className="personality-tabs"
            role="tablist"
            aria-label="Personality editing mode"
          >
            {(['guided', 'markdown'] as const).map((mode) => (
              <button
                key={mode}
                type="button"
                role="tab"
                id={`${id}-${mode}-tab`}
                aria-controls={`${id}-${mode}-panel`}
                aria-selected={view === mode}
                tabIndex={view === mode ? 0 : -1}
                disabled={busy}
                onClick={() => setView(mode)}
                onKeyDown={(event) => {
                  if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
                    event.preventDefault();
                    const next = mode === 'guided' ? 'markdown' : 'guided';
                    setView(next);
                    document.getElementById(`${id}-${next}-tab`)?.focus();
                  }
                }}
              >
                {mode === 'guided' ? 'Guided' : 'Markdown'}
              </button>
            ))}
          </div>
          <div
            role="tabpanel"
            id={`${id}-${view}-panel`}
            aria-labelledby={`${id}-${view}-tab`}
          >
            {view === 'guided' ? (
              <div className="personality-fields">
                {PERSONALITY_SECTIONS.map((section) => (
                  <div
                    key={section.id}
                    className={`personality-field personality-field--${section.id}`}
                  >
                    <label htmlFor={`${id}-${section.id}`}>
                      {section.title}
                    </label>
                    <p id={`${id}-${section.id}-hint`}>{section.hint}</p>
                    <textarea
                      id={`${id}-${section.id}`}
                      aria-describedby={`${id}-${section.id}-hint`}
                      className="field"
                      rows={
                        section.id === 'identity' || section.id === 'examples'
                          ? 3
                          : 2
                      }
                      value={fields[section.id]}
                      disabled={busy}
                      onChange={(event) =>
                        changeField(section.id, event.target.value)
                      }
                    />
                  </div>
                ))}
                <p className="personality-preservation">
                  Your existing text and any other sections stay in the
                  document. Open Markdown to edit the whole personality.
                </p>
              </div>
            ) : (
              <label className="personality-document">
                Personality document
                <textarea
                  className="field"
                  rows={18}
                  value={draft}
                  disabled={busy}
                  onChange={(event) => {
                    const content = event.target.value;
                    setDraft(content);
                    guideBase.current = content;
                    setFields(guidedFields(content));
                    setNotice('');
                  }}
                  spellCheck={false}
                />
              </label>
            )}
          </div>
          <div className="personality-actions">
            <span
              className={
                used > saved.maxChars
                  ? 'personality-error'
                  : 'personality-count'
              }
            >
              {used.toLocaleString()} / {saved.maxChars.toLocaleString()}{' '}
              characters
            </span>
            <button
              type="button"
              className="personality-save"
              disabled={busy || !dirty || !valid || conflict}
              onClick={() => void save()}
            >
              {saving ? 'Saving personality…' : 'Save personality'}
            </button>
          </div>
          {conflict && (
            <div className="personality-conflict">
              <p>
                Your draft is still here. Reloading replaces it with the latest
                saved personality.
              </p>
              <button
                type="button"
                className="personality-secondary"
                disabled={busy}
                onClick={() => setAttempt((value) => value + 1)}
              >
                Reload saved personality
              </button>
            </div>
          )}
          {notice && (
            <p role="status" className="personality-notice">
              {notice}
            </p>
          )}
          <details className="personality-preview">
            <summary>Preview personality document</summary>
            <pre aria-label="Personality preview">{draft}</pre>
          </details>
          {saved.path && (
            <p className="personality-file">
              Saved in <code>{saved.path}</code>
            </p>
          )}
        </>
      )}
    </section>
  );
}

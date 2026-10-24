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
import { PenIcon } from '../PenIcon';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';

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
type PersonalityPreset = 'quiet' | 'balanced' | 'driven';
const PRESETS: Record<PersonalityPreset, string> = {
  quiet: 'Wait for the owner to ask before suggesting more work. Keep answers short.',
  balanced: 'Suggest a useful next step when it helps, and keep answers concise.',
  driven: 'Take the lead on clearly useful next steps and briefly explain your reasoning, while staying within granted permissions.',
};
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
  compactProfile = false,
  initialScreen = 'profile',
  onBackToProfile,
  loadPersonality = daemon.agentPersonality,
  savePersonality = daemon.saveAgentPersonality,
}: {
  agentId: string;
  disabled?: boolean;
  compactProfile?: boolean;
  initialScreen?: 'profile' | 'editor';
  onBackToProfile?: () => void;
  loadPersonality?: Loader;
  savePersonality?: Saver;
}) {
  const id = useId();
  const [saved, setSaved] = useState<AgentPersonality | null>(null);
  const [draft, setDraft] = useState('');
  const [fields, setFields] = useState<GuidedFields>(() => guidedFields(''));
  const guideBase = useRef('');
  const [view, setView] = useState<'guided' | 'markdown'>('guided');
  const [screen, setScreen] = useState<'profile' | 'editor'>(initialScreen);
  const [versions, setVersions] = useState<AgentPersonality[]>([]);
  const lineNumbers = useRef<HTMLPreElement>(null);
  const [testPrompt, setTestPrompt] = useState('');
  const [previewResult, setPreviewResult] = useState<{
    content: string;
    prompt: string;
    text: string;
  } | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [previewError, setPreviewError] = useState<string | null>(null);
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
    setVersions([]);
    setScreen(initialScreen);
    setTestPrompt('');
    setPreviewResult(null);
    setPreviewLoading(false);
    setPreviewError(null);
  });
  useEffect(() => {
    const changed = (event: Event) => {
      if ((event as CustomEvent<string>).detail === agentId)
        setAttempt((value) => value + 1);
    };
    window.addEventListener('anima:personality-changed', changed);
    return () => window.removeEventListener('anima:personality-changed', changed);
  }, [agentId]);
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
      setVersions((current) => [saved, ...current].slice(0, 8));
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

  const currentPreset = ((): PersonalityPreset | null => {
    for (const preset of Object.keys(PRESETS) as PersonalityPreset[]) {
      if (personalitySection(draft, 'communication').trim() === PRESETS[preset])
        return preset;
    }
    return null;
  })();
  const choosePreset = (preset: PersonalityPreset) => {
    const next = updatePersonalitySection(draft, 'communication', PRESETS[preset]);
    setDraft(next);
    guideBase.current = next;
    setFields(guidedFields(next));
    setNotice('');
  };
  const tryPersonality = async () => {
    const content = draft;
    const prompt = testPrompt.trim();
    if (!prompt || previewLoading || busy) return;
    setPreviewLoading(true);
    setPreviewError(null);
    try {
      const result = await daemon.previewPersonality(agentId, { content, prompt });
      setPreviewResult({ content, prompt, text: result.text });
    } catch (caught) {
      setPreviewError(
        caught instanceof Error ? caught.message : 'Could not preview this personality.',
      );
    } finally {
      setPreviewLoading(false);
    }
  };

  if (screen === 'profile') {
    if (compactProfile) {
      return (
        <section
          className="settings-item personality-profile personality-profile-inline"
          aria-labelledby="personality-profile-title"
          aria-busy={loading || saving}
        >
          <span className="settings-item-icon" aria-hidden="true">
            <PenIcon name="smiley-fill" size={16} />
          </span>
          <div className="settings-item-text">
            <strong id="personality-profile-title">Personality</strong>
            <small>How it talks and how much it takes the lead</small>
          </div>
          {saved && (
            <fieldset
              className="personality-preset-group personality-preset-group-inline"
              disabled={busy}
            >
              <legend className="sr-only">Personality preset</legend>
              {(Object.keys(PRESETS) as PersonalityPreset[]).map((preset) => (
                <label
                  key={preset}
                  className={currentPreset === preset ? 'is-selected' : ''}
                >
                  <input
                    type="radio"
                    name={`${id}-preset`}
                    value={preset}
                    checked={currentPreset === preset}
                    onChange={() => choosePreset(preset)}
                  />
                  <span>
                    {preset[0].toUpperCase() + preset.slice(1)}
                  </span>
                </label>
              ))}
            </fieldset>
          )}
          {loading && <span role="status">Loading…</span>}
          {error && <span role="alert" className="personality-error">{error}</span>}
          {!loading && !saved && (
            <button
              type="button"
              className="personality-secondary personality-retry-inline"
              disabled={disabled}
              onClick={() => setAttempt((value) => value + 1)}
            >
              Retry
            </button>
          )}
          {saved && dirty && (
            <button
              type="button"
              className="personality-save personality-save-inline"
              disabled={busy || !valid || conflict}
              onClick={() => void save()}
            >
              {saving ? 'Saving…' : 'Save'}
            </button>
          )}
          {notice && (
            <span role="status" className="personality-notice-inline">
              {notice}
            </span>
          )}
        </section>
      );
    }

    return (
      <section className="settings-group personality-profile" aria-labelledby="personality-profile-title" aria-busy={loading || saving}>
        <div className="personality-heading">
          <h3 id="personality-profile-title" className="settings-group-title">Personality</h3>
          {saved && <span className="personality-source">{SOURCE_LABELS[saved.source]}</span>}
        </div>
        <p className="settings-group-help">How it talks and how much it takes the lead</p>
        {loading && <p role="status">Loading personality…</p>}
        {error && <p role="alert" className="personality-error">{error}</p>}
        {!loading && !saved && (
          <button type="button" className="personality-secondary" disabled={disabled} onClick={() => setAttempt((value) => value + 1)}>
            Retry loading personality
          </button>
        )}
        {saved && (
          <>
            <fieldset className="personality-preset-group" disabled={busy}>
              <legend className="sr-only">Personality preset</legend>
              {(Object.keys(PRESETS) as PersonalityPreset[]).map((preset) => (
                <label key={preset} className={currentPreset === preset ? 'is-selected' : ''}>
                  <input
                    type="radio"
                    name={`${id}-preset`}
                    value={preset}
                    checked={currentPreset === preset}
                    onChange={() => choosePreset(preset)}
                  />
                  <span>{preset[0].toUpperCase() + preset.slice(1)}</span>
                </label>
              ))}
            </fieldset>
            <div className="personality-profile-actions">
              <button type="button" className="personality-soul-link" onClick={() => { setView('markdown'); setScreen('editor'); }}>
                <span><strong>SOUL.md</strong><small>The personality file {agentId} reads before every reply</small></span>
                <span>Edit</span>
              </button>
              {dirty && (
                <button type="button" className="personality-save" disabled={busy || !valid || conflict} onClick={() => void save()}>
                  {saving ? 'Saving…' : 'Save personality'}
                </button>
              )}
            </div>
            {notice && <p role="status" className="personality-notice">{notice}</p>}
          </>
        )}
      </section>
    );
  }

  return (
    <section
      className="settings-group personality-editor personality-detail-editor"
      aria-labelledby={`${id}-title`}
      aria-busy={loading || saving}
    >
      <div className="personality-heading">
        <button
          type="button"
          className="personality-back"
          onClick={() => {
            if (onBackToProfile) onBackToProfile();
            else setScreen('profile');
          }}
        >
          <span aria-hidden="true">←</span> Profile
        </button>
        <h3 id={`${id}-title`} className="settings-group-title">SOUL.md</h3>
        {saved && (
          <span className="personality-source">
            {SOURCE_LABELS[saved.source]}
          </span>
        )}
      </div>
      <p className="settings-group-help">
        What {agentId} reads before every reply. Changes apply from the next
        message; a turn already in progress keeps its current personality.
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
          <div className="personality-file-bar">
            <span>
              <strong>{saved.path ?? 'SOUL.md'}</strong>
              <small>MARKDOWN · UTF-8 · {draft.split('\n').length} LINES</small>
            </span>
            <button type="button" className="personality-secondary" disabled={busy} onClick={() => choosePreset('balanced')}>
              Reset to Balanced
            </button>
            <label className="personality-import">
              Import…
              <input
                type="file"
                accept=".md,.markdown,.txt,text/markdown,text/plain"
                disabled={busy}
                onChange={(event) => {
                  const file = event.target.files?.[0];
                  event.target.value = '';
                  if (!file) return;
                  void file.text().then((content) => {
                    setDraft(content);
                    guideBase.current = content;
                    setFields(guidedFields(content));
                    setNotice('');
                  }).catch(() => setError('Could not read that file.'));
                }}
              />
            </label>
          </div>
          <div className="personality-editor-layout">
            <div className="personality-editor-main">
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
                    <span className="personality-markdown-editor">
                      <pre ref={lineNumbers} aria-hidden="true">
                        {Array.from({ length: draft.split('\n').length }, (_, index) => index + 1).join('\n')}
                      </pre>
                      <textarea
                        className="field"
                        rows={18}
                        value={draft}
                        disabled={busy}
                        onScroll={(event) => {
                          if (lineNumbers.current)
                            lineNumbers.current.scrollTop = event.currentTarget.scrollTop;
                        }}
                        onChange={(event) => {
                          const content = event.target.value;
                          setDraft(content);
                          guideBase.current = content;
                          setFields(guidedFields(content));
                          setNotice('');
                        }}
                        spellCheck={false}
                      />
                    </span>
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
            </div>
            <aside className="personality-editor-aside">
              <section className="personality-preview" aria-labelledby={`${id}-preview`}>
                <div className="personality-preview-head">
                  <h4 id={`${id}-preview`}>Preview</h4>
                  <span>Uses unsaved version</span>
                </div>
                <article aria-label="Personality preview" className="personality-rendered-preview">
                  <ReactMarkdown remarkPlugins={[remarkGfm]}>{draft}</ReactMarkdown>
                </article>
              </section>
              <section className="personality-try-it" aria-labelledby={`${id}-try-it`}>
                <div className="personality-preview-head">
                  <h4 id={`${id}-try-it`}>Try it</h4>
                  <span>Uses unsaved version · response only</span>
                </div>
                <form
                  onSubmit={(event) => {
                    event.preventDefault();
                    void tryPersonality();
                  }}
                >
                  <label className="sr-only" htmlFor={`${id}-try-prompt`}>
                    Ask something to test this personality
                  </label>
                  <textarea
                    id={`${id}-try-prompt`}
                    className="field"
                    rows={2}
                    maxLength={2000}
                    placeholder="Ask something to test…"
                    value={testPrompt}
                    onChange={(event) => setTestPrompt(event.target.value)}
                    disabled={busy || previewLoading}
                  />
                  <button
                    type="submit"
                    className="personality-secondary"
                    disabled={busy || previewLoading || !testPrompt.trim()}
                  >
                    {previewLoading ? 'Trying…' : 'Try it'}
                  </button>
                </form>
                {previewError && (
                  <p role="alert" className="personality-error">
                    {previewError}
                  </p>
                )}
                {previewResult?.content === draft &&
                  previewResult.prompt === testPrompt.trim() && (
                    <div className="personality-try-result" aria-live="polite">
                      <strong>{agentId}</strong>
                      <ReactMarkdown remarkPlugins={[remarkGfm]}>
                        {previewResult.text}
                      </ReactMarkdown>
                    </div>
                  )}
              </section>
              <section className="personality-versions" aria-labelledby={`${id}-versions`}>
                <h4 id={`${id}-versions`}>Versions</h4>
                <button type="button" disabled={!dirty || !saved} onClick={() => { setDraft(saved.content); setFields(guidedFields(saved.content)); guideBase.current = saved.content; }}>
                  <span><strong>Saved version</strong><small>Current source</small></span>
                  <span>Restore</span>
                </button>
                {versions.map((version, index) => (
                  <button key={`${version.revision}:${index}`} type="button" onClick={() => { setDraft(version.content); setFields(guidedFields(version.content)); guideBase.current = version.content; setNotice(''); }}>
                    <span><strong>Edited by you</strong><small>Saved earlier in this session</small></span>
                    <span>Restore</span>
                  </button>
                ))}
              </section>
            </aside>
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

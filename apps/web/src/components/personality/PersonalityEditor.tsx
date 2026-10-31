import { useEffect, useId, useRef, useState } from 'react';
import {
  DaemonHttpError,
  type AgentPersonality,
  type SaveAgentPersonality,
} from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';
import {
  personalitySection,
  updatePersonalitySection,
} from '../../lib/personality-document';
import './personality.css';
import { useResetOnChange } from '../../hooks/useResetOnChange';
import { PenIcon } from '../PenIcon';
import { BackButton } from '../ui-bits';
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
type PersonalityPreset = 'quiet' | 'balanced' | 'driven';
const PRESETS: Record<PersonalityPreset, string> = {
  quiet: 'Wait for the owner to ask before suggesting more work. Keep answers short.',
  balanced: 'Suggest a useful next step when it helps, and keep answers concise.',
  driven: 'Take the lead on clearly useful next steps and briefly explain your reasoning, while staying within granted permissions.',
};

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
  const [screen, setScreen] = useState<'profile' | 'editor'>(initialScreen);
  const [versions, setVersions] = useState<AgentPersonality[]>([]);
  const lineNumbers = useRef<HTMLPreElement>(null);
  const [mode, setMode] = useState<'write' | 'preview'>('write');
  const [testing, setTesting] = useState(false);
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
              <button type="button" className="personality-soul-link" onClick={() => setScreen('editor')}>
                <span><strong>SOUL.md</strong><small>The personality file this agent reads before every reply</small></span>
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

  const lineCount = draft.split('\n').length;
  return (
    <section
      className="soul-editor"
      aria-labelledby={`${id}-title`}
      aria-busy={loading || saving}
    >
      <BackButton
        onClick={() => {
          if (onBackToProfile) onBackToProfile();
          else setScreen('profile');
        }}
      >
        Profile
      </BackButton>
      <div className="soul-toolbar">
        <h3 id={`${id}-title`} className="soul-title">
          {saved?.path ?? 'SOUL.md'}
        </h3>
        {saved && (
          <span className="soul-state">
            {dirty ? 'Unsaved changes' : SOURCE_LABELS[saved.source]}
          </span>
        )}
        {saved && (
          <div className="soul-toolbar-actions">
            <div className="sk-segmented" role="group" aria-label="View">
              <button
                type="button"
                aria-pressed={mode === 'write'}
                onClick={() => setMode('write')}
              >
                Write
              </button>
              <button
                type="button"
                aria-pressed={mode === 'preview'}
                onClick={() => setMode('preview')}
              >
                Preview
              </button>
            </div>
            <button
              type="button"
              className="ui-button"
              aria-expanded={testing}
              onClick={() => setTesting((value) => !value)}
            >
              Test it
            </button>
            <button
              type="button"
              className="ui-button is-primary"
              disabled={busy || !dirty || !valid || conflict}
              onClick={() => void save()}
            >
              {saving ? 'Saving personality…' : 'Save personality'}
            </button>
          </div>
        )}
      </div>
      <p className="soul-help">
        What this agent reads before every reply. Changes apply from the next
        message; a reply already in progress keeps its current personality.
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
          className="ui-button"
          disabled={disabled}
          onClick={() => setAttempt((value) => value + 1)}
        >
          Retry loading personality
        </button>
      )}
      {saved && (
        <>
          {conflict && (
            <div className="personality-conflict">
              <p>
                Your draft is still here. Reloading replaces it with the latest
                saved personality.
              </p>
              <button
                type="button"
                className="ui-button"
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
          <div className="soul-body">
            {mode === 'write' ? (
              <label className="personality-document">
                <span className="sr-only">Personality document</span>
                <span className="personality-markdown-editor">
                  <pre ref={lineNumbers} aria-hidden="true">
                    {Array.from({ length: lineCount }, (_, index) => index + 1).join('\n')}
                  </pre>
                  <textarea
                    className="field"
                    aria-label="Personality document"
                    rows={22}
                    value={draft}
                    disabled={busy}
                    onScroll={(event) => {
                      if (lineNumbers.current)
                        lineNumbers.current.scrollTop =
                          event.currentTarget.scrollTop;
                    }}
                    onChange={(event) => {
                      setDraft(event.target.value);
                      setNotice('');
                    }}
                    spellCheck={false}
                  />
                </span>
              </label>
            ) : (
              <article
                aria-label="Personality preview"
                className="soul-preview chat-md"
              >
                <ReactMarkdown remarkPlugins={[remarkGfm]}>{draft}</ReactMarkdown>
              </article>
            )}
            {testing && (
              <aside className="soul-test" aria-labelledby={`${id}-try-it`}>
                <h4 id={`${id}-try-it`}>Test it</h4>
                <p>Uses this draft, saved or not. Only the reply is shown.</p>
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
                    rows={3}
                    maxLength={2000}
                    placeholder="Ask something to test…"
                    value={testPrompt}
                    onChange={(event) => setTestPrompt(event.target.value)}
                    disabled={busy || previewLoading}
                  />
                  <button
                    type="submit"
                    className="ui-button"
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
                    <div className="soul-test-result chat-md" aria-live="polite">
                      <ReactMarkdown remarkPlugins={[remarkGfm]}>
                        {previewResult.text}
                      </ReactMarkdown>
                    </div>
                  )}
              </aside>
            )}
          </div>
          <div className="soul-footer">
            <span
              className={used > saved.maxChars ? 'personality-error' : undefined}
            >
              Markdown · {lineCount} lines · {used.toLocaleString()} /{' '}
              {saved.maxChars.toLocaleString()} characters
            </span>
            <span className="soul-footer-actions">
              <button
                type="button"
                disabled={busy}
                onClick={() => choosePreset('balanced')}
              >
                Reset to Balanced
              </button>
              <label className="personality-import">
                Import file
                <input
                  type="file"
                  accept=".md,.markdown,.txt,text/markdown,text/plain"
                  className="sr-only"
                  disabled={busy}
                  onChange={(event) => {
                    const file = event.target.files?.[0];
                    event.target.value = '';
                    if (!file) return;
                    void file
                      .text()
                      .then((content) => {
                        setDraft(content);
                        setNotice('');
                      })
                      .catch(() => setError('Could not read that file.'));
                  }}
                />
              </label>
              <details className="soul-versions">
                <summary>Versions</summary>
                <div className="soul-versions-menu">
                  <button
                    type="button"
                    disabled={!dirty}
                    onClick={() => setDraft(saved.content)}
                  >
                    <strong>Saved version</strong>
                    <small>Current source</small>
                  </button>
                  {versions.map((version, index) => (
                    <button
                      key={`${version.revision}:${index}`}
                      type="button"
                      onClick={() => {
                        setDraft(version.content);
                        setNotice('');
                      }}
                    >
                      <strong>Edited by you</strong>
                      <small>Saved earlier in this session</small>
                    </button>
                  ))}
                </div>
              </details>
            </span>
          </div>
        </>
      )}
    </section>
  );
}

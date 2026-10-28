import { useState } from 'react';
import type { ConnectedSource } from '@animaos/sdk';
import { accountButton } from './presentation';

type Settings = NonNullable<ConnectedSource['settings']>;

export function SourceSearchSettings({
  source,
  disabled,
  onSave,
}: {
  source: ConnectedSource;
  disabled: boolean;
  onSave: (settings: Settings) => void;
}) {
  const [settings, setSettings] = useState<Settings>(source.settings!);
  const capabilities = source.capabilities;
  if (!capabilities || (!capabilities.history && !capabilities.attachmentFormats.length)) return null;
  return (
    <div className="space-y-3 border-t border-line pt-3 text-xs">
      {capabilities.history && (
        <div className="flex flex-wrap gap-3">
          <label>
            Search back
            <select className="field ml-2" value={settings.historyDepth} disabled={disabled}
              onChange={(event) => setSettings({ ...settings, historyDepth: event.target.value as Settings['historyDepth'] })}>
              <option value="recent">Recent inbox</option>
              <option value="days30">30 days</option>
              <option value="days90">90 days</option>
              <option value="days365">1 year</option>
            </select>
          </label>
          <label>
            Keep locally
            <select className="field ml-2" value={settings.retainedItemLimit} disabled={disabled}
              onChange={(event) => setSettings({ ...settings, retainedItemLimit: Number(event.target.value) as Settings['retainedItemLimit'] })}>
              <option value="100">Up to 100 messages</option>
              <option value="500">Up to 500 messages</option>
              <option value="2000">Up to 2,000 messages</option>
            </select>
          </label>
        </div>
      )}
      {capabilities.attachmentFormats.length > 0 && (
        <label className="flex gap-2">
          <input type="checkbox" checked={settings.attachmentText === 'supportedText'} disabled={disabled}
            onChange={(event) => setSettings({ ...settings, attachmentText: event.target.checked ? 'supportedText' : 'metadataOnly' })} />
          {source.kind === 'file' ? 'Include document text' : 'Include attachment text'}
        </label>
      )}
      <p className="text-ink-3">Local storage limits and unsupported files may leave some content out. PDF, Word and Excel text depends on device support.</p>
      {settings.historyDepth !== 'recent' && (
        <label className="flex gap-2">
          <input type="checkbox" checked={settings.historyPaused} disabled={disabled}
            onChange={(event) => setSettings({ ...settings, historyPaused: event.target.checked })} />
          Pause older-content import
        </label>
      )}
      <div className="flex flex-wrap gap-2">
        <button type="button" className={accountButton} disabled={disabled} onClick={() => onSave(settings)}>Save search settings</button>
        {source.settings?.historyDepth !== 'recent' && (
          <button type="button" className={accountButton} disabled={disabled}
            onClick={() => onSave({ ...settings, historyDepth: 'recent', historyPaused: false })}>Remove older content</button>
        )}
      </div>
      {source.historyCoverage && source.historyCoverage.state !== 'notRequested' && (
        <p role="status" className="text-ink-3">
          Older content: {({ pending: 'waiting to import', running: 'importing', paused: 'paused', complete: 'imported', partial: 'partially imported', unsupported: 'not supported', error: 'needs attention' } as Record<string, string>)[source.historyCoverage.state] ?? source.historyCoverage.state}
          {source.historyCoverage.reason ? ` · ${source.historyCoverage.reason === 'capacity' ? 'local storage limit reached' : 'some content is unavailable'}` : ''}
        </p>
      )}
      {source.attachmentCoverage && settings.attachmentText === 'supportedText' && (
        <p role="status" className="text-ink-3">
          Attachment text: {source.attachmentCoverage.indexed} indexed
          {source.attachmentCoverage.pending ? ` · ${source.attachmentCoverage.pending} waiting` : ''}
          {source.attachmentCoverage.partial || source.attachmentCoverage.omitted || source.attachmentCoverage.failed ? ' · some attachments are incomplete or unavailable' : ''}
        </p>
      )}
    </div>
  );
}

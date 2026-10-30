import type { ImageVersion, UploadedAttachment } from '@animaos/sdk';
import { useState } from 'react';
import { useAttachmentPreview } from '../../hooks/useAttachmentPreview';
import { revealInvisible } from '../../lib/skills';
import { MaximizeIcon, MinimizeIcon, XIcon } from '../icons';

export interface ImagePreviewSelection { versions: readonly ImageVersion[]; versionId: string }

export function ImageArtifactPreview({ selection, agentId, onSelect, onUseSaved, onClose, fullscreen, onToggleFullscreen }: {
  selection: ImagePreviewSelection;
  agentId: string;
  onSelect(versionId: string): void;
  onUseSaved?: (attachment: UploadedAttachment) => boolean;
  onClose(): void;
  fullscreen: boolean;
  onToggleFullscreen(trigger: HTMLElement): void;
}) {
  const [selectionError, setSelectionError] = useState(false);
  const index = selection.versions.findIndex((version) => version.id === selection.versionId);
  const version = selection.versions[index];
  const preview = useAttachmentPreview(agentId, version?.attachment.id, version?.attachment.mime, Boolean(version));
  const name = version ? revealInvisible(version.attachment.name).text : 'Image';
  return <>
    <header className="workspace-artifact-toolbar">
      <div className="workspace-artifact-file-info"><h2>{name}</h2><p>Image version {index + 1} · 1024 × 1024 PNG</p></div>
      <div className="workspace-artifact-actions" role="group" aria-label="Preview actions">
        {preview.url && version && <a className="workspace-artifact-action" href={preview.url} download={version.attachment.name} aria-label="Download PNG">Download</a>}
        <button type="button" className="workspace-artifact-action" aria-label={fullscreen ? 'Exit fullscreen' : 'Enter fullscreen'} onClick={(event) => onToggleFullscreen(event.currentTarget)}>{fullscreen ? <MinimizeIcon size={16} /> : <MaximizeIcon size={16} />}</button>
        <button type="button" className="workspace-artifact-action" aria-label="Close artifact preview" onClick={onClose}><XIcon size={16} /></button>
      </div>
    </header>
    <div className="flex items-center justify-between gap-2 border-b border-line px-4 py-2 text-sm">
      <button type="button" className="studio-tool-button" aria-label="Previous image version" disabled={index <= 0} onClick={() => onSelect(selection.versions[index - 1].id)}>Previous</button>
      <span>Version {index + 1} of {selection.versions.length}</span>
      <button type="button" className="studio-tool-button" aria-label="Next image version" disabled={index < 0 || index >= selection.versions.length - 1} onClick={() => onSelect(selection.versions[index + 1].id)}>Next</button>
    </div>
    <div className="flex min-h-0 flex-1 items-center justify-center overflow-auto p-4">
      {preview.url && <img src={preview.url} alt={name} onError={preview.imageFailed} className="max-h-full max-w-full object-contain" />}
      {preview.loading && <p role="status">Opening image…</p>}
      {preview.error && <p role="alert">This image is unavailable or its saved bytes changed. <button type="button" className="studio-tool-button" onClick={preview.retry}>Retry preview</button></p>}
    </div>
    {version && onUseSaved && <div className="border-t border-line p-4 text-sm">
      <button type="button" className="studio-tool-button" disabled={!preview.url} onClick={() => setSelectionError(!onUseSaved(version.attachment))}>Edit in chat</button>
      {selectionError && <p role="alert" className="mt-2 text-danger">The image could not be selected. Remove other image attachments in chat, then try again.</p>}
      <p className="mt-2 text-xs text-ink-3">Select this version, then describe your change in chat. Sending attaches it to your chat provider. You will separately review its transfer to OpenAI and API charges before the image edit. The original is preserved.</p>
    </div>}
  </>;
}

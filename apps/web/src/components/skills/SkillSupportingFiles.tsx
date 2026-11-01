import type { SkillSupportingFile } from '@animaos/sdk';
import { revealInvisible } from '../../lib/skills';

export function supportingFileBytes(file: SkillSupportingFile): number {
  return file.encoding === 'base64'
    ? Math.max(0, Math.floor(file.content.length * 3 / 4) - (file.content.endsWith('==') ? 2 : file.content.endsWith('=') ? 1 : 0))
    : new TextEncoder().encode(file.content).length;
}

/** Scripts stay literal text; binary content is never embedded or executed. */
export function SkillSupportingFiles({
  files = [],
}: {
  files?: SkillSupportingFile[];
}) {
  if (!files.length) return null;
  return (
    <section aria-label="Supporting files">
      <p>
        Supporting files ({files.length}). Approval covers every file. Scripts
        require separate execution permission.
      </p>
      {files.map((file) => (
        <details key={file.path}>
          <summary>{revealInvisible(file.path).text} ({supportingFileBytes(file).toLocaleString()} bytes{file.encoding === 'base64' ? ', binary asset' : ''})</summary>
          {file.encoding === 'base64' ? (
            <p>Binary asset preserved in the package. Its bytes are covered by approval.</p>
          ) : (
            <pre className="skill-body" aria-label={`File preview: ${file.path}`}>
              {revealInvisible(file.content.slice(0, 128 * 1024)).text}
              {file.content.length > 128 * 1024 ? '\nPreview limited to 128 KiB. The complete file is preserved.' : ''}
            </pre>
          )}
        </details>
      ))}
    </section>
  );
}

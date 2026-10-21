import type { SkillSupportingFile } from '@animaOS-SWARM/sdk';
import { revealInvisible } from '../../lib/skills';

/** Safe review: HTML, SVG and scripts remain literal visible text. */
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
        <div key={file.path}>
          <strong>{revealInvisible(file.path).text}</strong>
          <pre className="skill-body" aria-label={`File preview: ${file.path}`}>
            {revealInvisible(file.content).text}
          </pre>
        </div>
      ))}
    </section>
  );
}

import type { ReactNode } from 'react';

/**
 * The building blocks every settings page shares, so they read alike: a
 * section is a short title and one line of description, with its one action
 * on the right, then a card of rows. A row puts what a setting is on the
 * left and its control on the right. Styles live in settings-kit.css.
 */
export function SettingsSection({
  title,
  description,
  action,
  children,
  id,
  className = '',
}: {
  title: ReactNode;
  description?: ReactNode;
  /** The section's one action (Add, New…), beside the title. */
  action?: ReactNode;
  children?: ReactNode;
  /** The heading's id, for aria-labelledby. */
  id?: string;
  className?: string;
}) {
  return (
    <section className={`sk-section ${className}`.trim()} aria-labelledby={id}>
      <div className="sk-section-head">
        <div className="sk-section-text">
          <h2 id={id} className="sk-section-title">
            {title}
          </h2>
          {description && <p className="sk-section-desc">{description}</p>}
        </div>
        {action && <div className="sk-section-action">{action}</div>}
      </div>
      {children}
    </section>
  );
}

/** A bordered group of rows; rows are divided by hairlines. */
export function SettingsCard({
  children,
  className = '',
  as: Tag = 'div',
  label,
}: {
  children: ReactNode;
  className?: string;
  as?: 'div' | 'ul' | 'ol';
  /** Names a list for assistive tech. */
  label?: string;
}) {
  return (
    <Tag className={`sk-card ${className}`.trim()} aria-label={label}>
      {children}
    </Tag>
  );
}

/** One setting: what it is on the left, its control on the right. Extra
 *  content (details, an editor) goes under it, full width. */
export function SettingsRow({
  title,
  description,
  control,
  children,
  className = '',
  as: Tag = 'div',
}: {
  title: ReactNode;
  description?: ReactNode;
  control?: ReactNode;
  children?: ReactNode;
  className?: string;
  as?: 'div' | 'li' | 'label';
}) {
  return (
    <Tag className={`sk-row ${className}`.trim()}>
      <div className="sk-row-main">
        <div className="sk-row-text">
          <span className="sk-row-title">{title}</span>
          {description && <span className="sk-row-desc">{description}</span>}
        </div>
        {control && <div className="sk-row-control">{control}</div>}
      </div>
      {children}
    </Tag>
  );
}

/** What a card says when it has nothing to list yet. */
export function SettingsEmpty({ children }: { children: ReactNode }) {
  return <p className="sk-empty">{children}</p>;
}

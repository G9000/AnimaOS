import {
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { XIcon } from './icons';
import {
  ServiceDirectoryContext,
  isListed,
  serviceStatusLabel,
  serviceTone,
  type ServiceGroup,
} from './ServiceDirectory';

/**
 * One connection as a tile: its icon, name, where it stands and what it
 * does in a few words. Clicking the tile opens its settings, forms and
 * details in a drawer on the right, so the grid never moves. On a
 * connections page the tile shows only while connected or needing attention;
 * the rest wait in the catalogue, which can open the same drawer.
 */
export function ServiceRow({
  id,
  group,
  title,
  summary,
  status,
  icon,
  children,
}: {
  id: string;
  group: ServiceGroup;
  title: string;
  summary: string;
  status: string;
  icon: ReactNode;
  children: ReactNode;
}) {
  const directory = useContext(ServiceDirectoryContext);
  const drawer = useRef<HTMLDialogElement>(null);
  const [opened, setOpened] = useState(false);
  const open = () => {
    const element = drawer.current;
    if (!element) return;
    if (!element.open) {
      if (typeof element.showModal === 'function') element.showModal();
      else element.setAttribute('open', '');
    }
    setOpened(true);
  };
  const close = () => {
    const element = drawer.current;
    if (!element) return;
    if (typeof element.close === 'function') element.close();
    else element.removeAttribute('open');
    setOpened(false);
  };

  const iconRef = useRef(icon);
  iconRef.current = icon;
  const report = directory?.report;
  const forget = directory?.forget;
  useLayoutEffect(() => {
    report?.({ id, group, title, summary, status, icon: iconRef.current });
  }, [report, id, group, title, summary, status]);
  useLayoutEffect(() => () => forget?.(id), [forget, id]);

  const requested = directory?.request === id;
  const clear = directory?.clear;
  useEffect(() => {
    if (!requested) return;
    open();
    clear?.();
    // Opening is the whole response to a request.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [requested]);

  const listed = !directory || isListed(status);
  const state = serviceStatusLabel(status);
  return (
    <section
      aria-label={title}
      className="connections-row min-w-0"
      data-listed={listed || undefined}
      data-open={opened || undefined}
    >
      <div className="connections-row-tile">
        <span className="connections-row-icon">{icon}</span>
        <h3 className="connections-row-name">
          <button
            type="button"
            aria-haspopup="dialog"
            className="connections-row-open"
            onClick={open}
          >
            {title}
          </button>
        </h3>
        <span
          role="status"
          className="connections-row-status"
          data-tone={serviceTone(status)}
        >
          {state}
        </span>
        <span className="connections-row-what">{summary}</span>
      </div>
      <dialog
        ref={drawer}
        className="connections-drawer"
        aria-label={`${title} settings`}
        onClose={() => setOpened(false)}
        onClick={(event) => {
          // A click on the dimmed backdrop lands on the dialog itself.
          if (event.target === event.currentTarget) close();
        }}
      >
        <header className="connections-drawer-head">
          <span className="connections-row-icon">{icon}</span>
          <div>
            <p className="connections-drawer-title">{title}</p>
            <p>{summary}</p>
          </div>
          {opened && (
            <span
              className="connections-row-status"
              data-tone={serviceTone(status)}
            >
              {state}
            </span>
          )}
          <button
            type="button"
            className="connections-drawer-close"
            aria-label={`Close ${title}`}
            onClick={close}
          >
            <XIcon size={16} />
          </button>
        </header>
        <div className="connections-drawer-body">{children}</div>
      </dialog>
    </section>
  );
}

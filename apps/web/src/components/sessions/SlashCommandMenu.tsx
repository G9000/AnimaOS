import { useEffect, useRef } from 'react';
import {
  slashCommandGroup,
  type SlashCommand,
  type SlashCommandGroup,
} from '../../lib/slash-commands';

/** The composer's command list (spec §15.3), in sections; the input keeps
 *  focus and moves through it with the arrow keys. */
export function SlashCommandMenu({
  id,
  commands,
  activeName,
  onPick,
}: {
  id: string;
  commands: readonly SlashCommand[];
  activeName: string | null;
  onPick: (command: SlashCommand) => void;
}) {
  const list = useRef<HTMLUListElement>(null);
  useEffect(() => {
    if (!activeName) return;
    // jsdom has no scrollIntoView.
    list.current
      ?.querySelector<HTMLElement>('[aria-selected="true"]')
      ?.scrollIntoView?.({ block: 'nearest' });
  }, [activeName]);

  const sections: { group: SlashCommandGroup; commands: SlashCommand[] }[] =
    [];
  for (const command of commands) {
    const group = slashCommandGroup(command);
    const last = sections[sections.length - 1];
    if (last?.group === group) last.commands.push(command);
    else sections.push({ group, commands: [command] });
  }

  return (
    <div className="slash-menu">
      <ul ref={list} id={id} role="listbox" aria-label="Commands">
        {sections.map(({ group, commands: items }) => (
          <li key={group} role="presentation">
            {sections.length > 1 && (
              <span className="slash-menu-group" aria-hidden>
                {group}
              </span>
            )}
            <ul role="group" aria-label={group}>
              {items.map((command) => (
                <li
                  key={command.name}
                  id={`${id}-${command.name}`}
                  role="option"
                  aria-selected={command.name === activeName}
                  className="slash-menu-option"
                  onMouseDown={(event) => {
                    // Keep focus in the input.
                    event.preventDefault();
                    onPick(command);
                  }}
                >
                  <span className="slash-menu-name">/{command.name}</span>
                  <span className="slash-menu-description">
                    {command.description}
                  </span>
                  {command.placeholder && command.name === activeName && (
                    <span className="slash-menu-args">
                      {command.placeholder}
                    </span>
                  )}
                </li>
              ))}
            </ul>
          </li>
        ))}
      </ul>
      <footer className="slash-menu-keys" aria-hidden>
        <span>
          <kbd>↑</kbd>
          <kbd>↓</kbd> move
        </span>
        <span>
          <kbd>Tab</kbd> fill in
        </span>
        <span>
          <kbd>Enter</kbd> run
        </span>
        <span>
          <kbd>Esc</kbd> close
        </span>
      </footer>
    </div>
  );
}

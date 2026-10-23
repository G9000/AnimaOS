import {
  CHAT_BACKGROUNDS,
  useChatBackground,
  type ChatBackground,
} from '../lib/chat-background';

const BACKGROUNDS: Record<
  ChatBackground,
  { name: string; summary: string }
> = {
  full: { name: 'Full orb', summary: 'Behind the chat, as it is now.' },
  quiet: {
    name: 'Quiet',
    summary: 'Rests at the edge, so the text stays clear.',
  },
  off: { name: 'Off', summary: 'Plain dark, nothing moving.' },
};

/** How anima looks on this computer. It is kept here, not with the agent:
 *  chats and settings stay the same. */
export function AppearancePage() {
  const [background, setBackground] = useChatBackground();
  return (
    <div className="settings-form">
      <section className="settings-group">
        <h2 className="settings-group-title">Chat background</h2>
        <p className="settings-group-help">
          What sits behind the conversation. The orb always comes forward when
          you talk, and on an empty chat.
        </p>
        <fieldset className="settings-fieldset">
          <legend className="sr-only">Chat background</legend>
          <div className="settings-access-options">
            {CHAT_BACKGROUNDS.map((value) => {
              const { name, summary } = BACKGROUNDS[value];
              const selected = background === value;
              return (
                <div
                  key={value}
                  className={`settings-access-option${selected ? ' is-selected' : ''}`}
                >
                  <input
                    type="radio"
                    id={`chat-background-${value}`}
                    name="chat-background"
                    checked={selected}
                    onChange={() => setBackground(value)}
                  />
                  <label htmlFor={`chat-background-${value}`}>
                    <span
                      className="appearance-preview"
                      data-variant={value}
                      aria-hidden
                    />
                    <span className="settings-access-name">{name}</span>
                    <span className="settings-access-summary">{summary}</span>
                  </label>
                </div>
              );
            })}
          </div>
        </fieldset>
      </section>
    </div>
  );
}

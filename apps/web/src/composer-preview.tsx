// Temporary: renders the real Composer in the app's styles, without the daemon.
import { useState } from 'react';
import * as ReactDOM from 'react-dom/client';
import '@fontsource-variable/geist';
import '@fontsource-variable/geist-mono';
import '@fontsource-variable/fraunces';
import './styles.css';
import { Composer } from './components/ChatScreen';

function Preview() {
  const params = new URLSearchParams(location.search);
  const [draft, setDraft] = useState(params.get('draft') ?? '');
  const [plan, setPlan] = useState(false);
  const state = params.get('state');
  return (
    <div
      className="studio-shell companion-shell scenic-shell"
      style={{ minHeight: '100vh', display: 'flex', flexDirection: 'column', justifyContent: 'flex-end', background: '#0b0b0a' }}
    >
      <div data-main-chat="true" style={{ paddingBottom: 24 }}>
        <Composer
          agentName="Nova"
          draft={draft}
          setDraft={setDraft}
          sending={state === 'sending'}
          disabled={false}
          offline={state === 'offline'}
          runActive={state === 'running'}
          onStop={() => undefined}
          onSteer={() => undefined}
          onSend={() => setDraft('')}
          error={null}
          onDismissError={() => undefined}
          conversationKey="preview"
          sendAlways={params.get('main') !== '0'}
          commands={[
            { name: 'new', description: 'Start a new chat' },
            { name: 'help', description: 'What I can do' },
          ] as never}
          attachments={{ items: [], onAdd: () => undefined, onRemove: () => undefined }}
          mode={{ plan, canPlan: true, onChange: setPlan }}
          model={{
            provider: 'anthropic',
            current: 'claude-sonnet-5-5',
            onPick: () => undefined,
          }}
        />
      </div>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <Preview />,
);

import type { DaemonWorkspaceState } from '../lib/daemon-api';
import type { AgentDetail } from '../lib/types';
import { formatTokens } from '../components/ui-bits';

interface RuntimeOption {
  id: string;
  title: string;
  detail: string;
  current?: boolean;
}

/** Where the companion runs today and where it can run later. Only the
 *  current option works; the others say so instead of pretending. */
const HOME_OPTIONS: readonly RuntimeOption[] = [
  {
    id: 'local',
    title: 'This computer',
    detail:
      'Memory, skills, and connections live in the workspace folder here. Telegram and Discord answer while this computer is awake.',
    current: true,
  },
  {
    id: 'server',
    title: 'A server you own',
    detail:
      'Move the companion to a VPS so chats, check-ins, and connections keep running when this computer sleeps. This app then connects to it.',
  },
];

const COMMAND_OPTIONS: readonly RuntimeOption[] = [
  {
    id: 'workspace',
    title: 'In the workspace folder',
    detail:
      'Commands and file edits run on the same machine, inside the workspace folder, within the access set in Permissions.',
    current: true,
  },
  {
    id: 'sandbox',
    title: 'In an isolated sandbox',
    detail:
      'Run commands in a disposable container, so nothing outside it can change.',
  },
  {
    id: 'remote',
    title: 'On another machine over SSH',
    detail: 'Run commands on a build box or server you choose.',
  },
];

function OptionList({
  label,
  options,
}: {
  label: string;
  options: readonly RuntimeOption[];
}) {
  return (
    <ul className="runtime-options" aria-label={label}>
      {options.map((option) => (
        <li
          key={option.id}
          className={`runtime-option${option.current ? ' is-current' : ''}`}
          aria-current={option.current ? 'true' : undefined}
        >
          <div>
            <strong>{option.title}</strong>
            <p>{option.detail}</p>
          </div>
          <span className="runtime-option-state">
            {option.current ? 'In use' : 'Not available yet'}
          </span>
        </li>
      ))}
    </ul>
  );
}

function Fact({ label, value }: { label: string; value: string }) {
  return (
    <div className="runtime-fact">
      <dt>{label}</dt>
      <dd title={value}>{value}</dd>
    </div>
  );
}

export function RuntimePage({
  agent,
  workspace,
  online,
}: {
  agent: AgentDetail;
  workspace: DaemonWorkspaceState | null;
  online: boolean;
}) {
  const configured =
    workspace?.configured && workspace.workspace !== null
      ? workspace.workspace
      : null;
  return (
    <div className="runtime-page">
      <section className="runtime-section" aria-labelledby="runtime-home">
        <h2 id="runtime-home">Where {agent.name} lives</h2>
        <p className="runtime-help">
          Every surface reaches the same companion: this app, Telegram, and
          Discord all talk to one place.
        </p>
        <OptionList label={`Where ${agent.name} lives`} options={HOME_OPTIONS} />
      </section>

      <section className="runtime-section" aria-labelledby="runtime-commands">
        <h2 id="runtime-commands">Where commands run</h2>
        <OptionList label="Where commands run" options={COMMAND_OPTIONS} />
      </section>

      {configured && (
        <section className="runtime-section" aria-labelledby="runtime-workspace">
          <h2 id="runtime-workspace">Workspace</h2>
          <dl className="runtime-facts">
            <Fact label="Company" value={configured.companyName} />
            <Fact label="Mission" value={configured.mission} />
            <Fact label="Folder" value={configured.rootPath} />
          </dl>
        </section>
      )}

      <section className="runtime-section" aria-labelledby="runtime-status">
        <h2 id="runtime-status">Status</h2>
        <dl className="runtime-facts">
          <Fact label="Server" value={online ? 'Connected' : 'Offline'} />
          <Fact label="Companion" value={agent.status.toLowerCase()} />
          <Fact
            label="Created"
            value={new Date(agent.created_at_ms).toLocaleString([], {
              month: 'short',
              day: 'numeric',
              hour: '2-digit',
              minute: '2-digit',
            })}
          />
          <Fact
            label="Tokens used"
            value={formatTokens(agent.token_usage.total_tokens)}
          />
        </dl>
      </section>
    </div>
  );
}

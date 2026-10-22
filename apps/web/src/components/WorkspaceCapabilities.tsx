import { useEffect, useState } from 'react';
import type { DaemonCapabilities } from '@animaos/sdk';
import { daemon } from '../lib/daemon-api';

const groups = [
  [
    'workspace',
    'Files & workspace',
    'Read, create and maintain the work in your folder.',
  ],
  [
    'terminal',
    'Apps & terminals',
    'Run commands, background work and any app on this computer, such as coding agents.',
  ],
  [
    'memory',
    'Memory & context',
    'Keep useful context available for future work.',
  ],
  [
    'team',
    'Team coordination',
    'Let agents communicate and divide work within their authority.',
  ],
  [
    'research',
    'Research',
    'Gather source material with the registered research tools.',
  ],
  [
    'productivity',
    'Connected work',
    'Work with connected services when configured and authorized.',
  ],
  [
    'browser',
    'Browser tools',
    'Browse pages under an owner-selected policy and explicit tool grants.',
  ],
  [
    'documents',
    'Document tools',
    'Read selected supported documents with workspace access and an explicit tool grant.',
  ],
  ['utility', 'Utilities', 'Supporting tools for everyday agent work.'],
] as const;

function implementationStatusLabel(status: string) {
  switch (status) {
    case 'partial':
      return 'Partial implementation · setup required';
    case 'planned':
      return 'Planned · unavailable';
    default:
      return 'Implementation status unknown';
  }
}

export function WorkspaceCapabilities({ online }: { online: boolean }) {
  const [inventory, setInventory] = useState<DaemonCapabilities | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(online);
  const [refresh, setRefresh] = useState(0);
  const [query, setQuery] = useState('');

  useEffect(() => {
    let cancelled = false;
    setInventory(null);
    setError(null);
    setLoading(online);
    if (online) {
      void daemon.capabilities().then(
        (result) => {
          if (!cancelled) {
            setInventory(result);
            setLoading(false);
          }
        },
        (reason: unknown) => {
          if (!cancelled) {
            setError(
              reason instanceof Error
                ? reason.message
                : 'Capabilities could not be loaded.',
            );
            setLoading(false);
          }
        },
      );
    }
    return () => {
      cancelled = true;
    };
  }, [online, refresh]);

  const search = query.trim().toLowerCase();
  const tools =
    inventory?.tools.filter((tool) =>
      `${tool.name} ${tool.description} ${tool.category}`
        .toLowerCase()
        .includes(search),
    ) ?? [];
  const toolGroups = [
    ...groups.map(([category, label, description]) => ({
      category,
      label,
      description,
      members: tools.filter((tool) => tool.category === category),
    })),
    {
      category: 'other',
      label: 'Other tools',
      description: 'Registered tools from additional daemon categories.',
      members: tools.filter(
        (tool) => !groups.some(([category]) => tool.category === category),
      ),
    },
  ];

  return (
    <section aria-labelledby="capabilities-heading" className="tools-page">
      <header className="tools-head">
        <h2 id="capabilities-heading" className="sr-only">
          Capabilities
        </h2>
        <button
          type="button"
          disabled={!online || loading}
          onClick={() => setRefresh((value) => value + 1)}
          className="settings-item-action"
        >
          Refresh capabilities
        </button>
      </header>

      {!online ? (
        <div className="settings-list">
          <div className="settings-item">
            <div className="settings-item-text">
              <h3>Your daemon is offline</h3>
              <p>
                Connect to the daemon to inspect its current tools and
                persistence. Availability cannot be confirmed while offline.
              </p>
            </div>
          </div>
        </div>
      ) : loading ? (
        <p role="status" className="tools-note">
          Loading capabilities…
        </p>
      ) : error ? (
        <div role="alert" className="settings-list tools-error">
          <div className="settings-item">
            <div className="settings-item-text">
              <h3>Capabilities could not be loaded</h3>
              <p className="break-words">{error}</p>
              <p>Check the daemon connection and refresh to try again.</p>
            </div>
          </div>
        </div>
      ) : inventory ? (
        <>
          <section
            aria-labelledby="registered-tools-heading"
            className="tools-section"
          >
            <div className="tools-section-head">
              <div>
                <h3 id="registered-tools-heading">
                  Registered tools{' '}
                  <span className="tools-count">{inventory.tools.length}</span>
                </h3>
                <p>Inspect the actual tools behind your agents’ work.</p>
              </div>
              <input
                type="search"
                aria-label="Search capabilities"
                placeholder="Find a tool…"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                className="tools-search"
              />
            </div>
            <section
              className="tools-authority"
              aria-labelledby="capabilities-access"
            >
              <h3 id="capabilities-access">Tools follow your authority</h3>
              <p>
                These tools are registered in the daemon. Registration does not
                grant an agent access or confirm a service is connected. Each
                agent still needs the appropriate permissions and any listed
                requirements.
              </p>
            </section>
            {!tools.length && (
              <p className="tools-note">
                {search
                  ? 'No tools match your search.'
                  : 'No native tools are registered in this daemon.'}
              </p>
            )}
            <div className="tools-groups">
              {toolGroups.map(({ category, label, description, members }) =>
                members.length ? (
                  <section
                    key={category}
                    className="tools-group"
                    aria-label={label}
                  >
                    <header>
                      <h4>{label}</h4>
                      <span>
                        {members.length}{' '}
                        {members.length === 1 ? 'tool' : 'tools'}
                      </span>
                    </header>
                    <p className="tools-group-help">{description}</p>
                    <ul className="settings-list">
                      {members.map((tool) => (
                        <li key={tool.name} className="tools-row">
                          <p className="tools-row-name">{tool.name}</p>
                          <p className="tools-row-help">{tool.description}</p>
                          {tool.requirements.length > 0 && (
                            <div
                              className="tools-row-needs"
                              aria-label={`Requirements for ${tool.name}`}
                            >
                              {tool.requirements.map((requirement) => (
                                <span key={requirement}>{requirement}</span>
                              ))}
                            </div>
                          )}
                        </li>
                      ))}
                    </ul>
                  </section>
                ) : null,
              )}
            </div>
          </section>

          <section
            className="tools-section"
            aria-labelledby="capabilities-persistence"
          >
            <div className="tools-section-head">
              <div>
                <h3 id="capabilities-persistence">What carries forward</h3>
                <p>Saved records and running work have different lifetimes.</p>
              </div>
            </div>
            <dl className="settings-list">
              <div className="settings-item tools-fact">
                <dt>Workspace configuration</dt>
                <dd>{inventory.persistence.controlPlane}</dd>
              </div>
              <div className="settings-item tools-fact">
                <dt>Agent memory</dt>
                <dd>{inventory.persistence.memory}</dd>
              </div>
              <div className="settings-item tools-fact">
                <dt>Interrupted work</dt>
                <dd>
                  {inventory.persistence.executionJournal
                    ? 'Execution journal configured. Recovery depends on the operation and its authority.'
                    : 'Automatic run continuation is not available. Review interrupted work before starting it again.'}
                </dd>
              </div>
            </dl>
            <p className="tools-note">
              Agent task lists are saved separately in workspace files.
            </p>
            {inventory.limitations.length > 0 && (
              <ul className="tools-limits">
                {inventory.limitations.map((limitation) => (
                  <li key={limitation}>{limitation}</li>
                ))}
              </ul>
            )}
          </section>

          {inventory.extensions.length > 0 && (
            <section
              className="tools-section"
              aria-labelledby="additional-capabilities-heading"
            >
              <div className="tools-section-head">
                <div>
                  <h3 id="additional-capabilities-heading">
                    Additional capabilities
                  </h3>
                  <p>
                    Some modules have bounded implementations that require
                    setup; others remain planned.
                  </p>
                </div>
              </div>
              <ul className="settings-list">
                {inventory.extensions.map((extension) => (
                  <li key={extension.id} className="settings-item">
                    <div className="settings-item-text">
                      <h4>{extension.label}</h4>
                      <p>{extension.description}</p>
                    </div>
                    <span className="tools-status">
                      {implementationStatusLabel(extension.status)}
                    </span>
                  </li>
                ))}
              </ul>
            </section>
          )}
        </>
      ) : null}
    </section>
  );
}

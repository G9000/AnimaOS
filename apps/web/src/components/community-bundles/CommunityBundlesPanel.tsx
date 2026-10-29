import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import '../mcp/mcp.css';
import { useConnectorLifetime } from '../../hooks/useConnectorLifetime';
import {
  communityBundlesOwnerApi,
  type BundleReview,
  type BundleSummary,
  type BundleView,
  type CommunityManifest,
} from '../../lib/daemon-api';

export type CommunityBundlesOwnerApi = typeof communityBundlesOwnerApi;
const button = 'ui-button';
const inputClass = 'field';
const MANIFEST_BYTES = 128 * 1024;

function localInput(text: string): {
  issue: string | null;
  programs: { ref: string; args: unknown[] }[];
} {
  if (!text.trim())
    return { issue: 'Enter a bundle manifest to review.', programs: [] };
  if (new TextEncoder().encode(text).length > MANIFEST_BYTES)
    return { issue: 'The bundle manifest exceeds 128 KiB.', programs: [] };
  try {
    const value = JSON.parse(text) as {
      components?: { kind?: string; programRef?: unknown; args?: unknown }[];
    };
    if (!value || !Array.isArray(value.components))
      return {
        issue: 'Enter a JSON bundle manifest with components.',
        programs: [],
      };
    const mcp = value.components.filter(
      (part) => part?.kind === 'mcpHttp' || part?.kind === 'mcpStdio',
    );
    const skills = value.components.filter((part) => part?.kind === 'skill');
    if (value.components.length > 4 || mcp.length > 2 || skills.length > 2)
      return {
        issue:
          'Use at most four components: two external servers and two skills.',
        programs: [],
      };
    const programs = value.components
      .filter((part) => part?.kind === 'mcpStdio')
      .map((part) => ({
        ref: typeof part.programRef === 'string' ? part.programRef : '',
        args: Array.isArray(part.args) ? part.args : [],
      }));
    if (programs.some((program) => !program.ref))
      return {
        issue: 'Each local server needs a program reference.',
        programs: [],
      };
    return { issue: null, programs };
  } catch {
    return { issue: 'Enter a valid JSON bundle manifest.', programs: [] };
  }
}

function failure(caught: unknown): string {
  const status = (caught as { status?: number } | null)?.status;
  if (status === 400)
    return 'The bundle could not be reviewed. Check its manifest and program bindings.';
  if (status === 403) return 'This action is not available for this agent.';
  if (status === 409)
    return 'The bundle, review or available capacity changed. Refresh status and review your action again.';
  return 'The result could not be confirmed. Refresh status before taking another action.';
}

export function CommunityBundlesPanel({
  agentId,
  disabled = false,
  online = true,
  epoch: connectionEpoch = 0,
  permissionsUncertain = false,
  permissionsBusy = false,
  onGrantsChanged,
  onBusyChanged,
  onPermissionsUncertain,
  api = communityBundlesOwnerApi,
}: {
  agentId: string;
  disabled?: boolean;
  online?: boolean;
  epoch?: number;
  permissionsUncertain?: boolean;
  permissionsBusy?: boolean;
  onGrantsChanged?: (names: string[]) => void | Promise<void>;
  onBusyChanged?: (busy: boolean) => void;
  onPermissionsUncertain?: () => void;
  api?: CommunityBundlesOwnerApi;
}) {
  const lifetime = useConnectorLifetime(agentId, online, connectionEpoch);
  const [summaries, setSummaries] = useState<BundleSummary[]>([]);
  const [details, setDetails] = useState<Record<string, BundleView>>({});
  const [manifestText, setManifestText] = useState('');
  const [programBindings, setProgramBindings] = useState<
    Record<string, string>
  >({});
  const [trustedLocalCode, setTrustedLocalCode] = useState(false);
  const [target, setTarget] = useState<{
    id: string;
    version: number;
    name: string;
    componentStateDigest: string;
  } | null>(null);
  const [review, setReview] = useState<BundleReview | null>(null);
  const [loading, setLoading] = useState(true);
  const [inspecting, setInspecting] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [needsRefresh, setNeedsRefresh] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const generation = useRef(0);
  const previousLifetime = useRef(lifetime.token);
  if (previousLifetime.current !== lifetime.token) {
    previousLifetime.current = lifetime.token;
    generation.current++;
  }
  const readGeneration = useRef(0);
  const locked = useRef(false);
  const currentOwner = useRef(agentId);
  currentOwner.current = agentId;

  function current(epoch: number, owner: string) {
    return (
      lifetime.isCurrent(lifetime.token) &&
      generation.current === epoch &&
      currentOwner.current === owner
    );
  }

  function invalidateReview() {
    setReview(null);
    setTrustedLocalCode(false);
    setNotice(null);
  }

  async function refresh(signal?: AbortSignal, explicit = false) {
    if (
      !lifetime.isCurrent(lifetime.token) ||
      locked.current ||
      (explicit && permissionsBusy)
    )
      return;
    const epoch = generation.current;
    const read = ++readGeneration.current;
    const owner = agentId;
    setLoading(true);
    try {
      const [rows, names] = await Promise.all([
        api.list(owner, { signal }),
        explicit ? api.toolNames(owner) : Promise.resolve(undefined),
      ]);
      if (!current(epoch, owner) || read !== readGeneration.current) return;
      if (rows.some((row) => row.agentId !== owner))
        throw new Error('Owner mismatch');
      if (explicit && names) {
        onBusyChanged?.(true);
        try {
          await onGrantsChanged?.(names);
        } finally {
          if (lifetime.matches(lifetime.token)) onBusyChanged?.(false);
        }
        if (!current(epoch, owner) || read !== readGeneration.current) return;
      }
      setSummaries(rows);
      setDetails((values) =>
        Object.fromEntries(
          rows.flatMap((row) => {
            const value = values[row.id];
            return value &&
              value.version === row.version &&
              value.reviewedComponentStateDigest ===
                row.reviewedComponentStateDigest
              ? [[row.id, value]]
              : [];
          }),
        ),
      );
      if (explicit) setReview(null);
      setNeedsRefresh(false);
      setError(null);
    } catch {
      if (current(epoch, owner) && read === readGeneration.current) {
        setNeedsRefresh(true);
        setError(
          'Bundle status unavailable. Refresh to confirm saved components and permissions.',
        );
      }
    } finally {
      if (current(epoch, owner) && read === readGeneration.current)
        setLoading(false);
    }
  }

  useLayoutEffect(() => {
    setManifestText('');
    setProgramBindings({});
  }, [agentId, api]);

  useLayoutEffect(() => {
    generation.current += 1;
    locked.current = false;
    setSummaries([]);
    setDetails({});
    setTrustedLocalCode(false);
    setTarget(null);
    setReview(null);
    setBusy(false);
    setInspecting(null);
    setLoading(online);
    setNeedsRefresh(!online);
    setError(null);
    setNotice(null);
    const controller = new AbortController();
    if (online) void refresh(controller.signal);
    return () => {
      controller.abort();
      generation.current += 1;
      readGeneration.current += 1;
    };
    // Every read, review and mutation is fenced by the selected owner.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agentId, api, lifetime.token]);

  useEffect(() => {
    if (!review) return;
    const timer = window.setTimeout(
      () => {
        setReview(null);
        setNotice('This review expired. Review the exact inputs again.');
      },
      Math.max(0, review.expiresAtMs - Date.now()),
    );
    return () => window.clearTimeout(timer);
  }, [review]);

  function replace(value: BundleView) {
    if (value.agentId !== agentId) throw new Error('Owner mismatch');
    setSummaries((rows) =>
      rows.some((row) => row.id === value.id)
        ? rows.map((row) => (row.id === value.id ? value : row))
        : [...rows, value],
    );
    setDetails((values) => ({ ...values, [value.id]: value }));
  }

  async function perform<T>(
    action: () => Promise<T>,
    accepted: (value: T) => void | Promise<void>,
    affectsPermissions: boolean,
  ) {
    if (
      !lifetime.isCurrent(lifetime.token) ||
      disabled ||
      loading ||
      inspecting !== null ||
      needsRefresh ||
      permissionsUncertain ||
      permissionsBusy ||
      locked.current
    )
      return;
    locked.current = true;
    const epoch = generation.current;
    const owner = agentId;
    readGeneration.current += 1;
    setBusy(true);
    setError(null);
    setNotice(null);
    onBusyChanged?.(true);
    if (affectsPermissions) onPermissionsUncertain?.();
    try {
      const value = await action();
      if (current(epoch, owner)) await accepted(value);
    } catch (caught) {
      if (current(epoch, owner)) {
        const status = (caught as { status?: number } | null)?.status;
        setNeedsRefresh(affectsPermissions || status === 409 || status === 503);
        setReview(null);
        setError(failure(caught));
      }
    } finally {
      if (current(epoch, owner)) {
        locked.current = false;
        setBusy(false);
      }
      // Parent fencing survives unmount, navigation and unknown removal results.
      if (lifetime.matches(lifetime.token)) onBusyChanged?.(false);
    }
  }

  async function inspect(id: string) {
    if (
      !lifetime.isCurrent(lifetime.token) ||
      locked.current ||
      loading ||
      permissionsBusy
    )
      return;
    const epoch = generation.current;
    const owner = agentId;
    const read = ++readGeneration.current;
    setInspecting(id);
    try {
      const value = await api.get(owner, id);
      if (current(epoch, owner) && read === readGeneration.current) {
        if (value.id !== id || value.agentId !== owner)
          throw new Error('Owner mismatch');
        replace(value);
        if (
          review?.mode === 'update' &&
          value.id === review.bundleId &&
          (value.version !== review.expectedBundleVersion ||
            value.reviewedComponentStateDigest !==
              review.reviewedComponentStateDigest)
        )
          invalidateReview();
      }
    } catch {
      if (current(epoch, owner) && read === readGeneration.current) {
        setNeedsRefresh(true);
        setError(
          'Bundle details unavailable. Refresh before changing components.',
        );
      }
    } finally {
      if (current(epoch, owner) && read === readGeneration.current)
        setInspecting(null);
    }
  }

  async function acceptMutation(value: BundleView) {
    const epoch = generation.current;
    const owner = agentId;
    replace(value);
    setReview(null);
    await onGrantsChanged?.(value.toolNames);
    if (current(epoch, owner))
      setNotice(
        'Saved bundle state. Refresh External tools before changing its connections.',
      );
  }

  const input = localInput(manifestText);
  const programRefs = [
    ...new Set(input.programs.map((program) => program.ref)),
  ];
  const bindingValue = (ref: string) =>
    Object.prototype.hasOwnProperty.call(programBindings, ref)
      ? programBindings[ref]
      : '';
  const targetChanged =
    target !== null &&
    !summaries.some(
      (row) =>
        row.id === target.id &&
        row.version === target.version &&
        row.reviewedComponentStateDigest === target.componentStateDigest,
    );
  const blocked =
    !online ||
    disabled ||
    loading ||
    inspecting !== null ||
    busy ||
    needsRefresh ||
    permissionsUncertain ||
    permissionsBusy;
  const canReview =
    !blocked &&
    !input.issue &&
    !targetChanged &&
    programRefs.every((ref) => Boolean(bindingValue(ref).trim())) &&
    (programRefs.length === 0 || trustedLocalCode);
  const reviewUsable =
    review !== null &&
    !targetChanged &&
    review.expiresAtMs > Date.now() &&
    review.agentId === agentId &&
    (review.mode === 'install'
      ? target === null
      : target?.id === review.bundleId &&
        target.version === review.expectedBundleVersion);

  return (
    <section aria-label="Community bundles" className="mcp-panel mcp-bundles">
      <header className="mcp-head">
        <div>
          <h2 className="mcp-title">Community bundles</h2>
          <p className="mcp-lede">
            Ready-made sets of servers and skills. You review everything
            before installing; new servers stay off and skills wait for your
            approval.
          </p>
        </div>
        <button
          className="ui-button is-quiet"
          disabled={
            !online || busy || loading || inspecting !== null || permissionsBusy
          }
          onClick={() => void refresh(undefined, true)}
        >
          Refresh bundle status
        </button>
      </header>
      {loading && (
        <p role="status" className="text-xs text-ink-3">
          Reading saved bundles…
        </p>
      )}
      {error && (
        <p role="alert" className="text-sm text-ink">
          {error}
        </p>
      )}
      {notice && (
        <p role="status" className="text-xs text-ink-3">
          {notice}
        </p>
      )}
      {permissionsUncertain && (
        <p className="text-xs text-ink-3">
          After the current request finishes, refresh status to confirm
          permissions before further changes.
        </p>
      )}
      {disabled && (
        <p className="text-xs text-ink-3">
          Bundle changes are unavailable while this agent is busy or restricted.
        </p>
      )}
      <form
        className="space-y-3"
        onSubmit={(event) => {
          event.preventDefault();
          if (!canReview) return;
          const savedTarget = target;
          const savedText = manifestText;
          const bindings = Object.fromEntries(
            programRefs.map((ref) => [ref, programBindings[ref]]),
          );
          void perform(
            () =>
              api.review(
                agentId,
                savedTarget
                  ? {
                      manifestText: savedText,
                      programBindings: bindings,
                      bundleId: savedTarget.id,
                      expectedBundleVersion: savedTarget.version,
                    }
                  : { manifestText: savedText, programBindings: bindings },
              ),
            (value) => {
              if (
                value.agentId !== agentId ||
                (savedTarget
                  ? value.mode !== 'update' ||
                    value.bundleId !== savedTarget.id ||
                    value.expectedBundleVersion !== savedTarget.version
                  : value.mode !== 'install')
              )
                throw new Error('Review target mismatch');
              setReview(value);
            },
            false,
          );
        }}
      >
        <p className="text-xs text-ink-3">
          {target
            ? `Reviewing an update to ${target.name} at saved version ${target.version}.`
            : 'Reviewing a new installation.'}
        </p>
        {target && (
          <button
            className={button}
            type="button"
            disabled={blocked}
            onClick={() => {
              setTarget(null);
              invalidateReview();
            }}
          >
            Review a new installation
          </button>
        )}
        {targetChanged && (
          <p className="text-xs text-ink-3">
            The update target changed. Inspect it and choose Review update
            again; your entered content is kept.
          </p>
        )}
        <label className="block text-xs">
          Bundle manifest
          <textarea
            className={`${inputClass} min-h-40 font-mono`}
            value={manifestText}
            disabled={blocked}
            onChange={(event) => {
              setManifestText(event.target.value);
              invalidateReview();
            }}
          />
        </label>
        {manifestText && input.issue && (
          <p className="text-xs text-ink-3">{input.issue}</p>
        )}
        {programRefs.map((ref) => (
          <label key={ref} className="block text-xs">
            Executable for {ref}
            <input
              className={inputClass}
              value={bindingValue(ref)}
              maxLength={2048}
              disabled={blocked}
              onChange={(event) => {
                setProgramBindings((values) => ({
                  ...values,
                  [ref]: event.target.value,
                }));
                invalidateReview();
              }}
            />
          </label>
        ))}
        {input.programs.length > 0 && (
          <div className="space-y-2 text-xs text-ink-3">
            <p>
              Use preinstalled absolute executable paths. Each argument is
              literal, including empty strings. Installation does not download,
              authenticate or pin those programs.
            </p>
            {input.programs.map((program, index) => (
              <p
                key={`${program.ref}:${index}`}
                className="whitespace-pre-wrap break-all"
              >
                {program.ref}: {JSON.stringify(program.args)}
              </p>
            ))}
            <label className="flex items-start gap-2">
              <input
                type="checkbox"
                checked={trustedLocalCode}
                disabled={blocked}
                onChange={(event) => {
                  setTrustedLocalCode(event.target.checked);
                  setReview(null);
                  setNotice(null);
                }}
              />
              I trust these local programs and literal arguments
            </label>
          </div>
        )}
        <button className={button} type="submit" disabled={!canReview}>
          Review bundle
        </button>
      </form>
      {review && <ReviewPreview review={review} />}
      {review && (
        <button
          className={button}
          disabled={
            blocked ||
            !reviewUsable ||
            (review.mode === 'install' && summaries.length >= 16)
          }
          onClick={() => {
            if (!reviewUsable) return;
            const acceptedReview = review;
            void perform(
              () =>
                acceptedReview.mode === 'install'
                  ? api.install(agentId, {
                      reviewId: acceptedReview.id,
                      reviewDigest: acceptedReview.reviewDigest,
                    })
                  : api.update(agentId, acceptedReview.bundleId, {
                      expectedVersion: acceptedReview.expectedBundleVersion!,
                      reviewId: acceptedReview.id,
                      reviewDigest: acceptedReview.reviewDigest,
                    }),
              acceptMutation,
              true,
            );
          }}
        >
          {review.mode === 'install'
            ? 'Install reviewed bundle'
            : 'Save reviewed update'}
        </button>
      )}
      {summaries.length >= 16 && (
        <p className="text-xs text-ink-3">
          The workspace limit of 16 bundles is reached. A new installation
          cannot evict another bundle.
        </p>
      )}
      {!loading && summaries.length === 0 && (
        <p className="text-sm text-ink-3">No community bundles installed.</p>
      )}
      {summaries.map((row) => {
        const cached = details[row.id];
        const value =
          cached?.version === row.version &&
          cached.reviewedComponentStateDigest ===
            row.reviewedComponentStateDigest
            ? cached
            : undefined;
        return (
          <BundleCard
            key={`${agentId}:${row.id}`}
            row={row}
            value={value}
            blocked={blocked}
            reading={inspecting !== null}
            inspect={() => void inspect(row.id)}
            update={() => {
              if (!value || blocked) return;
              setTarget({
                id: value.id,
                version: value.version,
                name: value.name,
                componentStateDigest: value.reviewedComponentStateDigest,
              });
              if (!manifestText.trim())
                setManifestText(JSON.stringify(value.manifest, null, 2));
              invalidateReview();
            }}
            changeEnabled={() => {
              if (!value) return;
              void perform(
                () =>
                  api.setEnabled(agentId, row.id, {
                    expectedVersion: value.version,
                    reviewedComponentStateDigest:
                      value.reviewedComponentStateDigest,
                    enabled: !value.enabled,
                  }),
                acceptMutation,
                true,
              );
            }}
            remove={() => {
              if (!value) return;
              void perform(
                () =>
                  api.remove(agentId, row.id, {
                    expectedVersion: value.version,
                    reviewedComponentStateDigest:
                      value.reviewedComponentStateDigest,
                  }),
                async (outcome) => {
                  const epoch = generation.current;
                  const owner = agentId;
                  if (outcome.bundleId !== row.id || !outcome.removed)
                    throw new Error('Removal target mismatch');
                  setSummaries((rows) =>
                    rows.filter((saved) => saved.id !== row.id),
                  );
                  setDetails((values) => {
                    const next = { ...values };
                    delete next[row.id];
                    return next;
                  });
                  setReview(null);
                  await onGrantsChanged?.(outcome.toolNames);
                  if (current(epoch, owner))
                    setNotice(
                      'Owned components removed. Refresh External tools before changing other connections.',
                    );
                },
                true,
              );
            }}
          />
        );
      })}
    </section>
  );
}

function ManifestEvidence({ manifest }: { manifest: CommunityManifest }) {
  return (
    <div className="space-y-2 text-xs text-ink-3">
      <p>
        Requested native tools:{' '}
        {manifest.permissions.nativeToolNames.join(', ') || 'None'}. These
        requests are not grants.
      </p>
      {manifest.permissions.externalTools.map((tool, index) => (
        <p key={index}>
          {tool.component}: {tool.remoteName} — permission must be chosen in
          External tools.
        </p>
      ))}
      <p>
        Required capabilities:{' '}
        {manifest.compatibility.requiredCapabilities.join(', ') || 'None'}
      </p>
      <details>
        <summary>Inspect manifest and attribution claims</summary>
        <pre className="mt-2 max-h-64 overflow-auto whitespace-pre-wrap break-all">
          {JSON.stringify(manifest, null, 2)}
        </pre>
      </details>
      {manifest.attribution.map((entry, index) => (
        <p key={index} className="whitespace-pre-wrap break-all">
          {entry.component} · {entry.source} · {entry.revision} ·{' '}
          {entry.license} · {entry.notice}
        </p>
      ))}
      <p>
        Digests identify reviewed content. They do not authenticate publishers,
        pin external executable bytes or provide a sandbox.
      </p>
    </div>
  );
}

function ReviewPreview({ review }: { review: BundleReview }) {
  return (
    <section
      aria-label="Reviewed bundle"
      className="space-y-3 rounded-xl border border-line p-4"
    >
      <h3 className="break-words font-medium">
        Review ready: {review.manifest.name}
      </h3>
      <p className="break-all text-xs text-ink-3">
        Manifest digest: {review.manifestDigest}
      </p>
      <p className="break-all text-xs text-ink-3">
        Review digest: {review.reviewDigest}
      </p>
      <p className="text-xs text-ink-3">
        Supported by {review.compatibility.host} {review.compatibility.version};
        capabilities: {review.compatibility.capabilities.join(', ')}.
      </p>
      <p className="text-xs text-ink-3">
        Review expires at {new Date(review.expiresAtMs).toLocaleTimeString()}.
      </p>
      <ManifestEvidence manifest={review.manifest} />
      {review.components.map((part) => (
        <div
          key={part.key}
          className="space-y-1 border-t border-line pt-2 text-xs text-ink-3"
        >
          <p className="break-all">
            {part.key} — {part.action}
          </p>
          {part.kind === 'skill' ? (
            <>
              <p>
                Workspace-global skill: {part.slug}. Approval is separate and
                must retain the exact canonical content. An edited managed draft
                needs a fresh bundle review or an independent skill.
              </p>
              <p className="break-all">Source hash: {part.sourceHash}</p>
              <p className="break-all">Canonical hash: {part.canonicalHash}</p>
              <p className="break-all">
                Base hash: {part.baseHash ?? 'New skill'} · Current file hash:{' '}
                {part.actualHash ?? 'No current file'}
              </p>
              <details>
                <summary>Original skill content {part.key}</summary>
                <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-all">
                  {review.manifest.components.find(
                    (source) =>
                      source.kind === 'skill' && source.key === part.key,
                  )?.kind === 'skill'
                    ? (
                        review.manifest.components.find(
                          (source) =>
                            source.kind === 'skill' && source.key === part.key,
                        ) as { skillFile: string }
                      ).skillFile
                    : 'Unavailable'}
                </pre>
              </details>
              <details>
                <summary>Canonical skill content {part.key}</summary>
                <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-all">
                  {part.canonicalSkillFile}
                </pre>
              </details>
            </>
          ) : (
            <>
              <p className="break-all">
                Connection: {part.connectionId} · Previous:{' '}
                {part.previousConnectionId ?? 'None'} · Configuration digest:{' '}
                {part.configurationDigest}
              </p>
              <p className="whitespace-pre-wrap break-all">
                {part.kind === 'mcpHttp'
                  ? part.endpoint
                  : (part.stdio?.executable ?? 'Launch unavailable')}
              </p>
              {part.stdio && (
                <p className="whitespace-pre-wrap break-all">
                  Arguments: {JSON.stringify(part.stdio.args)}
                </p>
              )}
              <p>
                New connections stay disabled and ungranted. Starting and
                descriptor review are separate.
              </p>
            </>
          )}
        </div>
      ))}
    </section>
  );
}

function BundleCard({
  row,
  value,
  blocked,
  reading,
  inspect,
  update,
  changeEnabled,
  remove,
}: {
  row: BundleSummary;
  value?: BundleView;
  blocked: boolean;
  reading: boolean;
  inspect: () => void;
  update: () => void;
  changeEnabled: () => void;
  remove: () => void;
}) {
  const [confirmation, setConfirmation] = useState<{
    version: number;
    digest: string;
  } | null>(null);
  const unavailable =
    row.state === 'unavailable' ||
    row.state === 'removing' ||
    row.settlement !== null;
  const confirmed =
    confirmation?.version === row.version &&
    confirmation.digest === row.reviewedComponentStateDigest;
  return (
    <article
      aria-label={`Community bundle ${row.name}`}
      className="space-y-3 rounded-xl border border-line p-4"
    >
      <h3 className="break-words font-medium">{row.name}</h3>
      <p className="text-xs text-ink-3">
        {row.state} · Saved version {row.version} · {row.mcpCount} external
        servers · {row.skillCount} workspace-global skills
      </p>
      <p className="break-words text-xs text-ink-3">{row.description}</p>
      {unavailable && (
        <p className="text-xs text-ink-3">
          Settlement is unavailable or still active. Inspect and reconcile the
          exact state; completed tool effects are not undone.
        </p>
      )}
      <p className="text-xs text-ink-3">
        Disabling or removing owned skills affects all agents in this workspace.
      </p>
      <button className={button} disabled={reading} onClick={inspect}>
        Inspect bundle
      </button>
      {value && (
        <div className="space-y-2 text-xs text-ink-3">
          <ManifestEvidence manifest={value.manifest} />
          <p className="break-all">
            Manifest digest: {value.manifestDigest} · Component state:{' '}
            {value.reviewedComponentStateDigest}
          </p>
          {value.components.map((part) => (
            <p key={part.key} className="break-all">
              {part.key}:{' '}
              {part.kind === 'skill'
                ? `${part.status} · ${part.slug} · approved ${part.approvedHash ?? 'None'} · current ${part.actualHash ?? 'None'} · draft ${part.draftId ?? 'None'}`
                : `${part.runtimeState} · ${part.enabled ? 'enabled' : 'disabled'} · ${part.connectionId} · reviewed ${part.reviewedAliasNames.join(', ') || 'None'} · granted ${part.grantedAliasNames.join(', ') || 'None'}`}
            </p>
          ))}
          {value.predecessors.map((part) => (
            <p key={part.connectionId} className="break-all">
              Retained disabled predecessor: {part.connectionId}; saved
              permissions stay dormant until explicit removal.
            </p>
          ))}
          <div className="flex flex-wrap gap-3">
            <a href="#/connectors" className="underline">
              Manage external tools
            </a>
            <a href="#/skills" className="underline">
              Review skill drafts
            </a>
          </div>
          {value.skillCount > 0 && (
            <p>
              Managed draft approval must retain the exact reviewed canonical
              text. Edits require a fresh bundle review or an independent skill.
            </p>
          )}
          <p>
            These existing controls retain their separate exact review, grants
            and approval rules.
          </p>
        </div>
      )}
      <div className="flex flex-wrap gap-2">
        <button
          className={button}
          disabled={blocked || unavailable || !value}
          onClick={update}
        >
          Review update
        </button>
        <button
          className={button}
          disabled={blocked || unavailable || !value}
          onClick={changeEnabled}
        >
          {row.enabled ? 'Disable bundle' : 'Re-enable bundle'}
        </button>
        <button
          className={button}
          disabled={blocked || unavailable || !value}
          onClick={() =>
            setConfirmation({
              version: row.version,
              digest: row.reviewedComponentStateDigest,
            })
          }
        >
          Remove bundle
        </button>
      </div>
      {confirmation && (
        <div className="space-y-2 text-xs text-ink-3">
          <p>
            Remove only this bundle's displayed components and retained
            predecessors? Owned skills move to workspace trash. Completed tool
            effects are retained.
          </p>
          {!confirmed && (
            <p>
              The component state changed. Inspect it and choose Remove bundle
              again to reconfirm.
            </p>
          )}
          <button
            className={button}
            disabled={blocked || unavailable || !value || !confirmed}
            onClick={remove}
          >
            Confirm bundle removal
          </button>
          <button className={button} onClick={() => setConfirmation(null)}>
            Keep bundle
          </button>
        </div>
      )}
    </article>
  );
}

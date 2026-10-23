import { useCallback, useEffect, useState } from 'react';

/** The pages of spec §15.1; later milestones build the ones this release hides. */
export const HASH_PAGES = [
  'agents',
  'new-agent',
  'profile',
  'model',
  'appearance',
  'runtime',
  'approvals',
  'automations',
  'memory',
  'skills',
  'work',
  'files',
  'connectors',
  'usage',
  'logs',
  'health',
  'capabilities',
] as const;
export type HashPage = (typeof HASH_PAGES)[number];

export type HashRoute =
  | { kind: 'home' }
  | {
      kind: 'session';
      sessionId: string;
      /** Set for another agent's session, such as a helper's. */
      agentId?: string;
      /** An exact saved message, loaded as a bounded historical window. */
      messageId?: string;
    }
  | { kind: 'page'; page: HashPage };

/** Session ids (spec §3.1); agent ids fit the same pattern. */
const ROUTE_ID = /^[A-Za-z0-9._:-]{1,200}$/;

/** `#/s/<sessionId>`, `#/s/<agentId>/<sessionId>` for another agent's
 *  session, or `#/<page>`; anything else is a new chat. */
export function parseHashRoute(hash: string): HashRoute {
  const raw = hash.startsWith('#') ? hash.slice(1) : hash;
  const queryAt = raw.indexOf('?');
  const path = queryAt < 0 ? raw : raw.slice(0, queryAt);
  const query = new URLSearchParams(queryAt < 0 ? '' : raw.slice(queryAt + 1));
  const target = query.get('messageId');
  const messageId =
    query.getAll('messageId').length === 1 &&
    target?.trim() &&
    new TextEncoder().encode(target).length <= 256 &&
    !/[\p{Cc}\p{Cf}\p{Cs}]/u.test(target)
      ? target
      : undefined;
  const anchor = messageId ? { messageId } : {};
  if (path.startsWith('/s/')) {
    try {
      const parts = path
        .slice(3)
        .split('/')
        .map((part) => decodeURIComponent(part));
      if (parts.every((part) => ROUTE_ID.test(part))) {
        if (parts.length === 1)
          return { kind: 'session', sessionId: parts[0], ...anchor };
        if (parts.length === 2)
          return {
            kind: 'session',
            agentId: parts[0],
            sessionId: parts[1],
            ...anchor,
          };
      }
    } catch {
      // A malformed escape opens a new chat.
    }
    return { kind: 'home' };
  }
  const page = path.replace(/^\/+/, '');
  return (HASH_PAGES as readonly string[]).includes(page)
    ? { kind: 'page', page: page as HashPage }
    : { kind: 'home' };
}

export function formatHashRoute(route: HashRoute): string {
  switch (route.kind) {
    case 'home':
      return '#/';
    case 'session':
      return (
        (route.agentId
          ? `#/s/${encodeURIComponent(route.agentId)}/${encodeURIComponent(route.sessionId)}`
          : `#/s/${encodeURIComponent(route.sessionId)}`) +
        (route.messageId
          ? `?${new URLSearchParams({ messageId: route.messageId })}`
          : '')
      );
    case 'page':
      return `#/${route.page}`;
  }
}

export function sameRoute(left: HashRoute, right: HashRoute): boolean {
  return formatHashRoute(left) === formatHashRoute(right);
}

export type Navigate = (
  route: HashRoute,
  options?: { replace?: boolean },
) => void;

/** AppEntry owns this namespace, including malformed viewer links. */
export const isViewerHash = (hash: string) => hash.startsWith('#/shared');

/** The current hash route; a reload restores it and Back/Forward follow it. */
export function useHashRoute(): [HashRoute, Navigate] {
  const [route, setRoute] = useState<HashRoute>(() =>
    parseHashRoute(window.location.hash),
  );
  useEffect(() => {
    const update = () => {
      if (!isViewerHash(window.location.hash)) setRoute(parseHashRoute(window.location.hash));
    };
    window.addEventListener('hashchange', update);
    window.addEventListener('popstate', update);
    return () => {
      window.removeEventListener('hashchange', update);
      window.removeEventListener('popstate', update);
    };
  }, []);
  const navigate = useCallback<Navigate>((next, options = {}) => {
    // Owner effects can settle during the viewer transition or after unmount.
    // They must not replace the viewer URL before AppEntry can switch surfaces.
    if (isViewerHash(window.location.hash)) return;
    const hash = formatHashRoute(next);
    if (options.replace) {
      window.history.replaceState(window.history.state, '', hash);
    } else if (window.location.hash !== hash) {
      window.history.pushState(window.history.state, '', hash);
    }
    setRoute(parseHashRoute(hash));
  }, []);
  return [route, navigate];
}

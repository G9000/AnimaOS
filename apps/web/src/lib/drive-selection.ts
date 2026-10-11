const FILE_ID = /^[A-Za-z0-9_-]{1,256}$/;
export const INVALID_DRIVE_CONSENT =
  'Google returned an invalid consent link. Refresh Drive status before trying again.';

function selectedId(value: string): string | null {
  if (FILE_ID.test(value)) return value;
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return null;
  }
  if (
    url.protocol !== 'https:' ||
    url.username ||
    url.password ||
    (url.port && url.port !== '443')
  )
    return null;
  if (url.hostname !== 'docs.google.com' && url.hostname !== 'drive.google.com')
    return null;
  if (url.hostname === 'drive.google.com' && url.pathname === '/open') {
    const ids = url.searchParams.getAll('id');
    return ids.length === 1 && FILE_ID.test(ids[0]) ? ids[0] : null;
  }
  const path =
    url.hostname === 'drive.google.com'
      ? /^\/(?:file\/)?d\/([A-Za-z0-9_-]{1,256})(?:\/(?:view|edit|preview))?\/?$/
      : /^\/(?:document|spreadsheets|presentation|drawings)\/d\/([A-Za-z0-9_-]{1,256})(?:\/(?:view|edit|preview))?\/?$/;
  return path.exec(url.pathname)?.[1] ?? null;
}

/** Local parsing only: no file fetch, resource discovery or permission grant. */
export function parseDriveSelection(value: string): {
  ids: string[] | null;
  error: string | null;
} {
  const rows = value
    .split(/\r?\n/)
    .map((row) => row.trim())
    .filter(Boolean);
  if (rows.length < 1 || rows.length > 16)
    return {
      ids: null,
      error:
        'Select 1–16 file IDs or Google Drive/Docs file links, one per line.',
    };
  const ids: string[] = [];
  for (const row of rows) {
    const id = selectedId(row);
    if (!id)
      return {
        ids: null,
        error:
          'Use a file ID or an HTTPS Google Drive/Docs file link. Folders and other hosts are unavailable.',
      };
    if (ids.includes(id))
      return {
        ids: null,
        error: 'Duplicate selected files are not allowed. List each file once.',
      };
    ids.push(id);
  }
  return { ids, error: null };
}

/** Restrict anchors to actual Google OAuth authorization endpoints, not redirects. */
export function googleDriveConsentUrl(value: string): string | null {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return null;
  }
  if (
    url.origin !== 'https://accounts.google.com' ||
    url.username ||
    url.password ||
    url.hash
  )
    return null;
  if (url.pathname !== '/o/oauth2/v2/auth' && url.pathname !== '/o/oauth2/auth')
    return null;
  return url.href;
}
export function safeDriveError(error: unknown): string {
  if (error instanceof Error && error.message === INVALID_DRIVE_CONSENT)
    return INVALID_DRIVE_CONSENT;
  const status =
    typeof error === 'object' && error !== null && 'status' in error
      ? error.status
      : null;
  if (status === 404)
    return 'Drive is unavailable. Check the selected agent and use a daemon version that supports selected Drive files.';
  if (status === 400)
    return 'Check the selected file IDs. Choose 1–16 unique IDs.';
  if (status === 409)
    return 'Drive setup changed or needs fresh consent. Refresh status; disconnect before changing the selection.';
  if (status === 401 || status === 403)
    return 'Owner access is required to change this Drive connection.';
  return 'Drive request failed. Refresh status before trying again.';
}

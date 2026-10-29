const MAX_STORAGE_BYTES = 256 * 1024;

export function workspaceArtifactPath(
  value: string | undefined,
  literal = false,
): string | null {
  if (!value) return null;
  let path: string;
  try {
    const download = /^\/api\/workspace\/file\/download\?path=([^&#]+)$/.exec(
      value,
    );
    path = literal
      ? value
      : decodeURIComponent(download ? download[1].replace(/\+/g, ' ') : value);
  } catch {
    return null;
  }
  if (path.startsWith('./')) path = path.slice(2);
  if (
    new TextEncoder().encode(path).length > 1024 ||
    !/\.(?:html?|docx|xlsx|pptx)$/i.test(path) ||
    /[\\:?#]/.test(path) ||
    [...path].some(
      (character) =>
        character.charCodeAt(0) < 32 || character.charCodeAt(0) === 127,
    ) ||
    path.split('/').some((part) => !part || part === '.' || part === '..')
  )
    return null;
  return path;
}

/** Preview storage belongs to one file, never to the console's credentials. */
export function storageSnapshot(value: unknown): string | null {
  if (typeof value !== 'string' || value.length > MAX_STORAGE_BYTES)
    return null;
  try {
    const entries: unknown = JSON.parse(value);
    return Array.isArray(entries) &&
      entries.every(
        (entry: unknown) =>
          Array.isArray(entry) &&
          entry.length === 2 &&
          entry.every((part) => typeof part === 'string'),
      )
      ? value
      : null;
  } catch {
    return null;
  }
}

function scriptValue(value: unknown): string {
  return JSON.stringify(value).replaceAll('<', '\\u003c');
}

export function artifactDocument(
  source: string,
  nonce: string,
  snapshot: string,
): string {
  const policy =
    "default-src 'none'; script-src 'unsafe-inline' blob:; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; media-src data: blob:; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";
  // Install a file-specific Storage implementation before the artifact's scripts.
  // The opaque frame cannot read the console's cookies, DOM, or local storage.
  const bootstrap = `<script>(()=>{const entries=new Map(${scriptValue(JSON.parse(storageSnapshot(snapshot) ?? '[]'))});const nonce=${scriptValue(nonce)};const notify=()=>parent.postMessage({type:'anima-artifact-storage',nonce,snapshot:JSON.stringify([...entries])},'*');addEventListener('keydown',event=>{if(event.key==='Escape'){event.preventDefault();parent.postMessage({type:'anima-artifact-close',nonce},'*')}});const storage={get length(){return entries.size},key(i){return [...entries.keys()][i]??null},getItem(k){return entries.get(String(k))??null},setItem(k,v){entries.set(String(k),String(v));notify()},removeItem(k){entries.delete(String(k));notify()},clear(){entries.clear();notify()}};Object.defineProperty(window,'localStorage',{value:storage});})();</script>`;
  return `<!doctype html><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${policy}">${bootstrap}${source}`;
}

export function artifactWindowDocument(
  document: string,
  nonce: string,
  storageKey?: string,
): string {
  const attribute = document
    .replaceAll('&', '&amp;')
    .replaceAll('"', '&quot;')
    .replaceAll('<', '&lt;');
  // A trusted parent policy also blocks navigation of the executable child.
  // The child's own CSP cannot stop it navigating itself to a remote page.
  const policy =
    "default-src 'none'; frame-src 'none'; script-src 'unsafe-inline' blob:; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; media-src data: blob:; connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";
  const receive =
    storageKey === undefined
      ? "parent.postMessage(event.data,'*')"
      : `if(event.data.type==='anima-artifact-storage'){try{localStorage.setItem(${scriptValue(storageKey)},event.data.snapshot)}catch{}}`;
  return `<!doctype html><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${policy}"><title>Artifact preview</title><style>html,body,iframe{margin:0;width:100%;height:100%;border:0}iframe{display:block}</style><iframe title="Artifact page" sandbox="allow-scripts allow-downloads allow-modals" srcdoc="${attribute}"></iframe><script>const frame=document.querySelector('iframe');addEventListener('message',event=>{if(event.source!==frame.contentWindow||event.data?.nonce!==${scriptValue(nonce)})return;if(event.data.type==='anima-artifact-close'||(event.data.type==='anima-artifact-storage'&&typeof event.data.snapshot==='string'&&event.data.snapshot.length<=${MAX_STORAGE_BYTES})){${receive}}});</script>`;
}

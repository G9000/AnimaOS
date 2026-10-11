import { readFile, realpath, stat } from 'node:fs/promises';
import { extname, isAbsolute, relative, resolve } from 'node:path';

export const DIST = resolve(__dirname, '../../../../web/dist');
const MIME: Record<string, string> = {
  '.html': 'text/html',
  '.js': 'text/javascript',
  '.css': 'text/css',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.woff2': 'font/woff2',
  '.ico': 'image/x-icon',
  '.json': 'application/json',
};
function contained(root: string, path: string) {
  const rel = relative(root, path);
  return (
    rel !== '..' &&
    !rel.startsWith(`..${process.platform === 'win32' ? '\\' : '/'}`) &&
    !isAbsolute(rel)
  );
}
export async function staticAsset(pathname: string, root = DIST) {
  const decoded = decodeURIComponent(pathname);
  if (
    !decoded.startsWith('/') ||
    decoded.includes('\\') ||
    decoded.includes('\0') ||
    decoded.includes(':') ||
    decoded.split('/').includes('..')
  )
    throw new Error(`Unsafe asset path: ${pathname}`);
  const canonical = await realpath(root);
  const requested = resolve(
    canonical,
    `.${decoded === '/' ? '/index.html' : decoded}`,
  );
  if (!contained(canonical, requested)) throw new Error('Asset leaves dist');
  const file = await realpath(requested);
  if (!contained(canonical, file) || !(await stat(file)).isFile())
    throw new Error('Asset is not a contained file');
  const contentType = MIME[extname(file)];
  if (!contentType) throw new Error(`Undeclared asset MIME: ${file}`);
  return { body: await readFile(file), contentType };
}

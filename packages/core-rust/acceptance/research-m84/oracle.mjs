import { createHash } from 'node:crypto';
import { isDeepStrictEqual } from 'node:util';

const labels = [
  ['Minimum stable Rust', 'minimumStable'],
  ['Manifest entry', 'manifestEntry'],
  ['Automatic migration command', 'migrationCommand'],
];
const toolNames = ['web_search', 'web_fetch', 'write_file', 'read_file'];
const validReportPath = (value) =>
  typeof value === 'string' &&
  value.length <= 64 &&
  /^[A-Za-z0-9][A-Za-z0-9._-]*\.md$/.test(value) &&
  !/^(?:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])\./i.test(value);
const validVersion = (value) =>
  typeof value === 'string' &&
  /^(?:0|[1-9]\d{0,2})\.(?:0|[1-9]\d{0,2})\.(?:0|[1-9]\d{0,2})$/.test(value);
const sha256 = (text) => createHash('sha256').update(text).digest('hex');
const numbered = (text) =>
  text
    .split('\n')
    .map((line, index) => `${String(index + 1).padStart(6)}| ${line}`)
    .join('\n');
const escapeRegex = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

function hasFact(text, key, expected, fetched = false) {
  if (typeof text !== 'string') return false;
  const visible = fetched
    ? text.replace(/&quot;|&#34;|&#x22;/gi, '"')
    : text.replace(/<!--[\s\S]*?-->/g, '').replace(/<[^>]*>/g, '');
  if (key === 'minimumStable')
    return (
      new RegExp(
        `(?:^|[^0-9A-Za-z.])${escapeRegex(expected)}(?![0-9A-Za-z]|\\.[0-9]|-[A-Za-z0-9])`,
      ).test(visible) &&
      (fetched ||
        !new RegExp(
          `${escapeRegex(expected)}\\s+(?:is|was)\\s+not\\b`,
          'i',
        ).test(visible))
    );
  if (key === 'manifestEntry') {
    const parsed = /^edition\s*=\s*"(\d{4})"$/.exec(expected);
    return (
      !!parsed &&
      new RegExp(`\\bedition\\s*=\\s*"${parsed[1]}"(?![0-9])`).test(visible)
    );
  }
  if (key === 'migrationCommand') {
    if (expected !== 'cargo fix --edition') return false;
    return /(?:^|[^a-z0-9-])cargo\s+fix\s+--edition(?![a-z0-9-])/.test(visible);
  }
  return false;
}

function sourceIdentity(raw) {
  try {
    const url = new URL(raw);
    if (
      url.protocol !== 'https:' ||
      url.username ||
      url.password ||
      url.port ||
      !['doc.rust-lang.org', 'blog.rust-lang.org'].includes(url.hostname)
    )
      return null;
    url.hash = '';
    return url.href;
  } catch {
    return null;
  }
}

function publicResultUrl(raw) {
  try {
    const url = new URL(raw);
    return (
      url.protocol === 'https:' && !url.username && !url.password && !url.port
    );
  } catch {
    return false;
  }
}

function parseRows(text, reject) {
  const lines = text.split('\n');
  if (Buffer.byteLength(text, 'utf8') > 8192 || lines.length > 100)
    reject('report', 'bounds');
  const first = lines.find((line) => line.trim());
  if (!/^# [^#\n]{1,80}$/.test(first ?? '')) reject('report', 'title');
  const at = lines.findIndex((line) =>
    /^\|\s*Question\s*\|\s*Answer\s*\|\s*Source\s*\|\s*$/.test(line),
  );
  if (
    at < 0 ||
    !/^\|\s*:?-{3,}:?\s*\|\s*:?-{3,}:?\s*\|\s*:?-{3,}:?\s*\|\s*$/.test(
      lines[at + 1] ?? '',
    )
  ) {
    reject('report', 'table');
    return [];
  }
  const rows = [];
  for (const line of lines.slice(at + 2)) {
    if (!line.trim().startsWith('|')) break;
    const cells = line
      .trim()
      .split('|')
      .slice(1, -1)
      .map((cell) => cell.trim());
    if (cells.length !== 3) reject('report', 'table-row');
    else rows.push(cells);
  }
  if (rows.length !== 3 || rows.some((row, i) => row[0] !== labels[i][0]))
    reject('report', 'rows');
  if (!/\b(?:limit|review|test)\b/i.test(lines.slice(at + 5).join(' ')))
    reject('report', 'limitations');
  return rows;
}

/** Grade one controller-captured M84 case. The report itself supplies no provenance. */
export function evaluateResearch(input) {
  const checks = {};
  const reasons = [];
  const reject = (boundary, reason, severity = 'fail') => {
    checks[boundary] = false;
    reasons.push({ boundary, reason, severity });
  };
  if (!input || typeof input !== 'object')
    return {
      status: 'fail',
      checks: { input: false },
      reasons: [{ boundary: 'input', reason: 'missing', severity: 'fail' }],
    };
  const { report, expectations, operations, restart, cleanup } = input;
  if (
    !report ||
    typeof report.text !== 'string' ||
    !report.text ||
    typeof report.fileText !== 'string' ||
    !report.fileText
  )
    reject('report', 'missing');
  if (
    !expectations ||
    typeof expectations.query !== 'string' ||
    !validReportPath(expectations.path) ||
    !validVersion(expectations.minimumStable) ||
    labels.some(
      ([, key]) => typeof expectations[key] !== 'string' || !expectations[key],
    )
  )
    reject('expectations', 'invalid');
  if (!Array.isArray(operations)) reject('operations', 'missing');
  if (reasons.length) return { status: 'fail', checks, reasons };

  const text = report.text;
  const rows = parseRows(text, reject);
  if (
    report.fileText !== text ||
    report.fileSha256 !== sha256(report.fileText) ||
    report.fileBytes !== Buffer.byteLength(report.fileText, 'utf8')
  )
    reject('report', 'file-witness-mismatch');

  let modelCount = 0;
  let finalCount = 0;
  let lastPhase = 0;
  let phaseModel = 0;
  const calls = [];
  for (const entry of operations) {
    if (entry?.type === 'model') {
      modelCount++;
      if (entry.producer !== 'primary' || entry.retry !== false)
        reject('budget', 'secondary-or-retry');
      if (finalCount) reject('chronology', 'model-after-final');
    } else if (entry?.type === 'tool') {
      calls.push(entry);
      if (finalCount) reject('chronology', 'tool-after-final');
      const phase = toolNames.indexOf(entry.name) + 1;
      if (!phase) reject('tools', 'forbidden-tool');
      if (entry.status !== 'success') reject('tools', 'unsuccessful-tool');
      if (
        entry.approval !==
        (entry.name === 'read_file' ? 'not_required' : 'allow_once')
      )
        reject('approval', 'wrong-decision');
      if (phase < lastPhase) reject('chronology', 'out-of-order-tool');
      if (phase > lastPhase) {
        if (modelCount <= phaseModel) reject('chronology', 'same-model-batch');
        lastPhase = phase;
      }
      phaseModel = modelCount;
    } else if (entry?.type === 'final') {
      finalCount++;
      if (modelCount <= phaseModel)
        reject('chronology', 'final-without-model-round');
      if (
        typeof entry.text !== 'string' ||
        !entry.text.includes(expectations.path)
      )
        reject('final', 'missing-report-name');
      if (
        /\b(?:I|we)\s+(?:ran|executed|performed)\s+(?:cargo|tests?|migration|compilation)\b/i.test(
          entry.text ?? '',
        ) ||
        /\b(?:I|we)\s+(?:migrated|compiled|tested)\s+(?:the|a|this)\s+(?:project|crate|code)\b/i.test(
          entry.text ?? '',
        )
      )
        reject('final', 'unsupported-execution-claim');
    } else reject('operations', 'unknown-entry');
  }
  if (modelCount > 6 || modelCount === 0)
    reject('budget', 'model-attempt-count');
  if (finalCount !== 1 || operations.at(-1)?.type !== 'final')
    reject('final', 'terminal-final');

  const named = (name) => calls.filter((entry) => entry.name === name);
  const search = named('web_search');
  const fetches = named('web_fetch');
  const writes = named('write_file');
  const reads = named('read_file');
  if (
    search.length !== 1 ||
    fetches.length < 1 ||
    fetches.length > 2 ||
    writes.length !== 1 ||
    reads.length > 1 ||
    calls.length !==
      search.length + fetches.length + writes.length + reads.length
  )
    reject('tools', 'operation-counts');
  if (
    search[0]?.args?.query !== expectations.query ||
    search[0]?.args?.num_results !== 3
  )
    reject('search', 'query-or-count');
  const returned = search[0]?.output?.urls;
  if (
    !Array.isArray(returned) ||
    !returned.length ||
    returned.length > 3 ||
    returned.some((url) => typeof url !== 'string' || !publicResultUrl(url))
  )
    reject('search', 'invalid-results');
  const returnedUrls = Array.isArray(returned) ? returned : [];
  const fetched = new Map();
  for (const fetch of fetches) {
    const raw = fetch.args?.url;
    const identity = typeof raw === 'string' ? sourceIdentity(raw) : null;
    if (
      !identity ||
      !returnedUrls.includes(raw) ||
      fetched.has(identity) ||
      !Number.isInteger(fetch.args?.max_length) ||
      fetch.args.max_length < 1 ||
      fetch.args.max_length > 20000 ||
      typeof fetch.output?.text !== 'string' ||
      !fetch.output.text.trim()
    )
      reject('fetch', 'unreturned-duplicate-or-empty');
    if (identity)
      fetched.set(
        identity,
        typeof fetch.output?.text === 'string' ? fetch.output.text : '',
      );
  }
  for (let i = 0; i < labels.length; i++) {
    const [label, key] = labels[i];
    const row = rows[i];
    if (!row || row[0] !== label) continue;
    if (!hasFact(row[1], key, expectations[key]))
      reject('facts', `${key}-answer`);
    const link = /^\[[^\]]+\]\((https:\/\/[^\s)]+)\)$/.exec(row[2]);
    const identity = link ? sourceIdentity(link[1]) : null;
    if (!identity || !fetched.has(identity))
      reject('sources', `${key}-citation`);
    else if (!hasFact(fetched.get(identity), key, expectations[key], true))
      reject('sources', `${key}-insufficient-evidence`);
  }
  const write = writes[0];
  const read = reads[0];
  if (
    write?.args?.file_path !== expectations.path ||
    write.args?.content !== text
  )
    reject('write', 'path-or-content');
  if (
    !read ||
    read.args?.file_path !== expectations.path ||
    read.args?.offset !== 0 ||
    read.args?.limit !== 200 ||
    read.output?.text !== numbered(text)
  )
    reject('readback', 'missing-or-incomplete', 'partial');
  if (!write || (read && operations.indexOf(read) < operations.indexOf(write)))
    reject('chronology', 'read-before-write');
  if (
    read &&
    operations.findIndex((entry) => entry?.type === 'final') <
      operations.indexOf(read)
  )
    reject('chronology', 'final-before-read');

  if (
    !restart ||
    !restart.before ||
    !restart.after ||
    typeof restart.quietSeconds !== 'number' ||
    !Number.isFinite(restart.quietSeconds) ||
    restart.quietSeconds < 5 ||
    restart.newActions !== 0 ||
    !isDeepStrictEqual(restart.before, restart.after) ||
    restart.before.reportSha256 !== report.fileSha256 ||
    !Array.isArray(restart.before.messages) ||
    !Array.isArray(restart.before.sources) ||
    !Array.isArray(restart.before.receipts) ||
    !restart.before.usage
  )
    reject('restart', 'changed-or-unproven', 'partial');
  if (cleanup?.state !== 'confirmed')
    reject('cleanup', 'unconfirmed', 'partial');
  const status = reasons.some((reason) => reason.severity === 'fail')
    ? 'fail'
    : reasons.length
      ? 'partial'
      : 'pass';
  if (status === 'pass') checks.complete = true;
  return { status, checks, reasons };
}

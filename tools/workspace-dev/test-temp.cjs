const { createHash } = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');

const marker = '.anima-nx-temp.json';
// Capture before this plugin changes TMPDIR. Reusing a daemon worker must not
// nest the next invocation inside an already cleaned invocation.
const temporaryRoot = fs.realpathSync(os.tmpdir());

function runDirectory(context, create) {
  const root = fs.realpathSync(context.workspaceRoot);
  const workspace =
    'anima-nx-' + createHash('sha256').update(root).digest('hex').slice(0, 16);
  const name =
    'run-' + createHash('sha256').update(context.id).digest('hex').slice(0, 24);
  let directory = temporaryRoot;
  for (const part of [workspace, name]) {
    directory = path.join(directory, part);
    let stat = fs.lstatSync(directory, { throwIfNoEntry: false });
    if (!stat) {
      if (!create) return null;
      try {
        fs.mkdirSync(directory);
      } catch (error) {
        // Shared workspace parents may be created by a concurrent Nx process.
        // The final run directory remains exclusive to this invocation.
        if (part === name || error.code !== 'EEXIST') throw error;
      }
      stat = fs.lstatSync(directory);
    } else if (create && part === name) {
      throw new Error(`Temporary run directory already exists: ${directory}`);
    }
    if (
      stat.isSymbolicLink() ||
      !stat.isDirectory() ||
      fs.realpathSync(directory) !== directory
    ) {
      throw new Error(
        `Refusing symlink or redirected temporary directory: ${directory}`,
      );
    }
  }
  return { root, directory };
}

// Nx propagates these environment mutations to task processes, including when
// the plugin runs in an isolated worker or the daemon. No task arguments change.
function preTasksExecution(_options, context) {
  const { root, directory } = runDirectory(context, true);
  fs.writeFileSync(
    path.join(directory, marker),
    JSON.stringify({ id: context.id, root }),
    { flag: 'wx' },
  );
  for (const key of ['TMP', 'TEMP', 'TMPDIR']) process.env[key] = directory;
}

function postTasksExecution(_options, context) {
  try {
    const run = runDirectory(context, false);
    if (!run) return;
    const ownership = JSON.parse(
      fs.readFileSync(path.join(run.directory, marker), 'utf8'),
    );
    if (ownership.id !== context.id || ownership.root !== run.root) {
      throw new Error(
        `Temporary directory ownership mismatch: ${run.directory}`,
      );
    }
    // The resolved absolute path has been checked component by component. Never
    // derive cleanup targets from TMPDIR, task output, file age, or a glob.
    fs.rmSync(run.directory, {
      recursive: true,
      force: true,
      maxRetries: 3,
      retryDelay: 100,
    });
  } catch (error) {
    // Preserve the original task result if Windows still has a file open.
    console.warn(
      `[test-temp] Could not clean temporary files: ${error.message}`,
    );
  }
}

module.exports = {
  name: 'anima-test-temp',
  preTasksExecution,
  postTasksExecution,
};

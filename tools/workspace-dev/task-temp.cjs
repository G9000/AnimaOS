const { createHash } = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');

const marker = '.anima-task-temp.json';
// Nested workspace scripts share the original OS temp root, not their parent's
// disposable run directory. Each command still gets a separate owned directory.
const temporaryRoot = fs.realpathSync(
  process.env.ANIMAOS_TASK_TEMP_ROOT || os.tmpdir(),
);

function runDirectory(context, create) {
  const root = fs.realpathSync(context.workspaceRoot);
  const workspace =
    'anima-task-' +
    createHash('sha256').update(root).digest('hex').slice(0, 16);
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
        // Shared workspace parents may be created by a concurrent command.
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

function prepareTask(context) {
  const { root, directory } = runDirectory(context, true);
  fs.writeFileSync(
    path.join(directory, marker),
    JSON.stringify({ id: context.id, root }),
    { flag: 'wx' },
  );
  return {
    TMP: directory,
    TEMP: directory,
    TMPDIR: directory,
    ANIMAOS_TASK_TEMP_ROOT: temporaryRoot,
  };
}

function cleanupTask(context) {
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
      `[task-temp] Could not clean temporary files: ${error.message}`,
    );
  }
}

module.exports = {
  prepareTask,
  cleanupTask,
};

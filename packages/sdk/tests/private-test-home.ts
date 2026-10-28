import { execFile } from 'node:child_process';
import { lstat, mkdir, mkdtemp, realpath, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, dirname, isAbsolute, join } from 'node:path';
import { promisify } from 'node:util';

export interface PrivateTestHome {
  readonly home: string;
  readonly workspace: string;
  readonly profile: string;
  cleanup(): Promise<void>;
}

// The .NET Framework overload passes this descriptor to CreateDirectoryW.
// Neither operation changes the permissions of an existing directory.
const windowsHomeScript = `
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath($env:ANIMA_TEST_HOME_ROOT)
$privateHomePath = [IO.Path]::GetFullPath($env:ANIMA_TEST_HOME_PATH)
if ([IO.Path]::GetDirectoryName($privateHomePath) -ne $root) { throw 'Private home must be a direct child of its owned root' }
if (([IO.File]::GetAttributes($root) -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Owned root is a reparse point' }
if ($env:ANIMA_TEST_HOME_OPERATION -eq 'create') {
  if (Test-Path -LiteralPath $privateHomePath) { throw 'Private home already exists' }
  $user = [Security.Principal.WindowsIdentity]::GetCurrent().User
  $security = New-Object System.Security.AccessControl.DirectorySecurity
  $security.SetSecurityDescriptorSddlForm('O:' + $user.Value + 'D:P(A;OICI;FA;;;' + $user.Value + ')(A;OICI;FA;;;SY)')
  [void][IO.Directory]::CreateDirectory($privateHomePath, $security)
  if (([IO.File]::GetAttributes($privateHomePath) -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Private home is a reparse point' }
  $actual = [IO.Directory]::GetAccessControl($privateHomePath)
  if ($actual.GetOwner([Security.Principal.SecurityIdentifier]).Value -ne $user.Value -or !$actual.AreAccessRulesProtected) { throw 'Private home owner or DACL protection is incorrect' }
  $rules = @($actual.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]))
  if ($rules.Count -ne 2) { throw 'Private home must have exactly two access rules' }
  $trustees = @($rules | ForEach-Object { $_.IdentityReference.Value } | Sort-Object)
  $expected = @($user.Value, 'S-1-5-18') | Sort-Object
  if (($trustees -join ',') -ne ($expected -join ',')) { throw 'Private home has unexpected trustees' }
  foreach ($rule in $rules) {
    if ($rule.IsInherited -or [int]$rule.AccessControlType -ne 0 -or [int]$rule.FileSystemRights -ne 0x001f01ff -or [int]$rule.InheritanceFlags -ne 3 -or [int]$rule.PropagationFlags -ne 0) { throw 'Private home access rule is incorrect' }
  }
} elseif ($env:ANIMA_TEST_HOME_OPERATION -eq 'inspect') {
  $pending = New-Object 'System.Collections.Generic.Stack[string]'
  $pending.Push($root)
  while ($pending.Count -gt 0) {
    $path = $pending.Pop()
    $attributes = [IO.File]::GetAttributes($path)
    if (($attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Refusing cleanup through a reparse point' }
    if (($attributes -band [IO.FileAttributes]::Directory) -ne 0) {
      foreach ($child in [IO.Directory]::EnumerateFileSystemEntries($path)) { $pending.Push($child) }
    }
  }
} else { throw 'Unknown private home operation' }
`;

async function windowsHome(
  operation: 'create' | 'inspect',
  root: string,
  home: string,
): Promise<void> {
  const systemRoot = process.env.SYSTEMROOT ?? process.env.WINDIR;
  if (!systemRoot || !isAbsolute(systemRoot))
    throw new Error('Windows system directory is unavailable');
  const systemEnvironment = Object.fromEntries(
    Object.entries(process.env).filter(([key]) =>
      /^(PATH|SYSTEMROOT|WINDIR|TEMP|TMP)$/i.test(key),
    ),
  );
  await promisify(execFile)(
    join(systemRoot, 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe'),
    [
      '-NoProfile',
      '-NonInteractive',
      '-EncodedCommand',
      Buffer.from(windowsHomeScript, 'utf16le').toString('base64'),
    ],
    {
      shell: false,
      windowsHide: true,
      timeout: 15000,
      maxBuffer: 1024 * 1024,
      env: {
        ...systemEnvironment,
        ANIMA_TEST_HOME_OPERATION: operation,
        ANIMA_TEST_HOME_ROOT: root,
        ANIMA_TEST_HOME_PATH: home,
      },
    },
  );
}

export async function createPrivateTestHome(
  prefix = 'anima-private-test-',
): Promise<PrivateTestHome> {
  if (!/^[a-z0-9][a-z0-9-]*-$/i.test(prefix))
    throw new Error('Invalid private test home prefix');
  const parent = await realpath(tmpdir());
  const root = await mkdtemp(join(parent, prefix));
  const identity = await lstat(root, { bigint: true });
  const home = join(root, 'home');
  const workspace = join(home, 'workspace');
  const profile = join(home, 'profile');
  let removed = false;

  async function verifyOwnedRoot() {
    const current = await lstat(root, { bigint: true });
    if (
      dirname(root) !== parent ||
      !basename(root).startsWith(prefix) ||
      !current.isDirectory() ||
      current.isSymbolicLink() ||
      (await realpath(root)) !== root ||
      current.dev !== identity.dev ||
      current.ino !== identity.ino
    )
      throw new Error('Private test home root was replaced or redirected');
  }

  async function cleanup() {
    if (removed) return;
    await verifyOwnedRoot();
    if (process.platform === 'win32') await windowsHome('inspect', root, home);
    await verifyOwnedRoot();
    await rm(root, { recursive: true, maxRetries: 3, retryDelay: 100 });
    removed = true;
  }

  try {
    await verifyOwnedRoot();
    if (process.platform === 'win32') await windowsHome('create', root, home);
    else await mkdir(home, { mode: 0o700 });
    await mkdir(workspace, { mode: 0o700 });
    await mkdir(profile, { mode: 0o700 });
    return { home, workspace, profile, cleanup };
  } catch (error) {
    await cleanup();
    throw error;
  }
}

#!/usr/bin/env bun
// Packages an already-built native-install binary. No dependency installation,
// application launch, signing, publication, or user configuration changes occur.
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { deflateSync } from 'node:zlib';

export const PACKAGER_VERSION = '0.11.8';
export const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const TEMPLATE = join(ROOT, 'tools/release/nsis-installer.nsi');
const PLATFORMS = {
  'win32-x64': { target: 'x86_64-pc-windows-msvc', formats: ['nsis'] },
  'linux-x64': { target: 'x86_64-unknown-linux-gnu', formats: ['deb', 'appimage'] },
  'darwin-arm64': { target: 'aarch64-apple-darwin', formats: ['app', 'dmg'] },
  'darwin-x64': { target: 'x86_64-apple-darwin', formats: ['app', 'dmg'] },
};

function version() {
  const manifest = readFileSync(join(ROOT, 'Cargo.toml'), 'utf8');
  const workspace = manifest.split('[workspace.package]')[1]?.split(/^\[/m)[0];
  const value = workspace?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (!value || !/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(value)) {
    throw new Error('Cannot read a release version from [workspace.package] in Cargo.toml');
  }
  return value;
}

export function configuration(platform, arch, binary, output, assets) {
  const selected = PLATFORMS[`${platform}-${arch}`];
  if (!selected) throw new Error(`Native packaging is not configured for ${platform}-${arch}`);
  const config = {
    name: 'anima-native', productName: 'Anima', identifier: 'dev.animaos.anima',
    version: version(), publisher: 'AnimaOS', authors: ['animaOS contributors'],
    description: 'Anima local agent runtime', category: 'Productivity',
    homepage: 'https://github.com/G9000/animaOS-SWARM',
    targetTriple: selected.target, formats: selected.formats,
    binaries: [{ path: basename(binary).replace(/\.exe$/i, ''), main: true }],
    binariesDir: dirname(binary), outDir: output,
    icons: [join(assets, 'anima-512.png')],
  };
  if (platform === 'win32') {
    config.windows = { allowDowngrades: false };
    config.nsis = {
      installMode: 'currentUser', template: TEMPLATE,
      installerIcon: join(assets, 'anima.ico'),
      languages: ['English'], displayLanguageSelector: false,
      // Deliberately no appdataPaths: uninstall always retains personal data.
    };
    config.resources = [{ src: join(assets, 'anima.ico'), target: 'anima.ico' }];
  } else if (platform === 'darwin') {
    config.macos = { backgroundApp: true, minimumSystemVersion: '11.0' };
  } else {
    config.deb = {
      packageName: 'anima', section: 'utils',
      depends: ['libc6 (>= 2.35)', 'libgcc-s1', 'xdg-utils'],
    };
  }
  return config;
}

export function validateBuildInfo(info, expectedVersion) {
  if (info?.nativeInstall !== true) throw new Error('The binary must be built with native-install enabled');
  if (info?.bundledUi !== true) throw new Error('The binary must be built with bundled-ui enabled');
  if (info.version !== expectedVersion) throw new Error(`Binary version ${info.version} does not match workspace version ${expectedVersion}`);
}

export function validateReleasePlan(plan, expectedVersion = version()) {
  const tag = plan?.announcement_tag;
  if (![expectedVersion, `v${expectedVersion}`, `anima/${expectedVersion}`].includes(tag)) {
    throw new Error(`Release plan tag ${JSON.stringify(tag)} does not match workspace version ${expectedVersion}`);
  }
  return tag;
}

function checkBinary(binary) {
  const result = spawnSync(binary, ['build-info'], {
    cwd: ROOT, encoding: 'utf8', timeout: 10000, maxBuffer: 1024 * 1024,
  });
  if (result.error || result.status !== 0) {
    throw new Error('The supplied binary must support the state-free build-info command; rebuild with bundled-ui,native-install');
  }
  let info;
  try { info = JSON.parse(result.stdout); }
  catch { throw new Error('The supplied binary returned invalid build-info JSON'); }
  validateBuildInfo(info, version());
}

function checkTemplate() {
  const template = readFileSync(TEMPLATE, 'utf8');
  const guard = template.match(/!macro CheckIfAppIsRunning\b([\s\S]*?)!macroend/)?.[1] ?? '';
  if (!guard.includes('FindProcess') || !guard.includes('SetErrorLevel 3') ||
      !guard.includes('Abort') || template.includes('KillProcess') ||
      /RMDir\s+\/r\b/i.test(template)) {
    throw new Error('The NSIS template must abort for running Anima and preserve user data');
  }
}

// A code-drawn app icon matching the runtime's neutral Anima ring. PNG and ICO
// files are generated below target/ so packaging needs no image conversion tool.
function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit++) crc = (crc >>> 1) ^ ((crc & 1) ? 0xedb88320 : 0);
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function png(size) {
  const pixels = Buffer.alloc((size * 4 + 1) * size);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const radius = Math.hypot(x + 0.5 - size / 2, y + 0.5 - size / 2);
      const coverage = Math.max(0, Math.min(1, size * 0.40 - radius + 0.5)) *
        Math.max(0, Math.min(1, radius - size * 0.28 + 0.5));
      const offset = y * (size * 4 + 1) + 1 + x * 4;
      pixels.set([138, 143, 152, Math.round(coverage * 255)], offset);
    }
  }
  const chunk = (type, data) => {
    const contents = Buffer.concat([Buffer.from(type), data]);
    const result = Buffer.alloc(contents.length + 8);
    result.writeUInt32BE(data.length, 0);
    contents.copy(result, 4);
    result.writeUInt32BE(crc32(contents), result.length - 4);
    return result;
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(size, 0); header.writeUInt32BE(size, 4);
  header[8] = 8; header[9] = 6;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk('IHDR', header), chunk('IDAT', deflateSync(pixels)), chunk('IEND', Buffer.alloc(0))]);
}

export function generateAssets(directory) {
  mkdirSync(directory, { recursive: true });
  writeFileSync(join(directory, 'anima-512.png'), png(512));
  const images = [16, 32, 48, 64, 128, 256].map((size) => ({ size, bytes: png(size) }));
  const header = Buffer.alloc(6 + 16 * images.length);
  header.writeUInt16LE(1, 2); header.writeUInt16LE(images.length, 4);
  let offset = header.length;
  images.forEach(({ size, bytes }, index) => {
    const entry = 6 + index * 16;
    header[entry] = size === 256 ? 0 : size; header[entry + 1] = header[entry];
    header.writeUInt16LE(1, entry + 4); header.writeUInt16LE(32, entry + 6);
    header.writeUInt32LE(bytes.length, entry + 8); header.writeUInt32LE(offset, entry + 12);
    offset += bytes.length;
  });
  writeFileSync(join(directory, 'anima.ico'), Buffer.concat([header, ...images.map((image) => image.bytes)]));
}

function tool() {
  const local = join(ROOT, 'target/tools/bin', process.platform === 'win32' ? 'cargo-packager.exe' : 'cargo-packager');
  const command = process.env.CARGO_PACKAGER_BIN || (existsSync(local) ? local : 'cargo-packager');
  const result = spawnSync(command, ['--version'], { encoding: 'utf8', cwd: ROOT });
  if (result.error || result.status !== 0) {
    throw new Error(`Install the packaging CLI first: cargo install cargo-packager --version ${PACKAGER_VERSION} --locked --root target/tools`);
  }
  if (result.stdout.trim() !== `cargo-packager ${PACKAGER_VERSION}`) {
    throw new Error(`Expected cargo-packager ${PACKAGER_VERSION}; found ${result.stdout.trim()}`);
  }
  return command;
}

export function defaultBinaryPath(platform = process.platform) {
  return resolve(ROOT, 'target/cargo/release', platform === 'win32' ? 'anima.exe' : 'anima');
}

export function main(args = process.argv.slice(2)) {
  let check = false;
  let binary = defaultBinaryPath();
  let output = join(ROOT, 'target/native-installers');
  for (let index = 0; index < args.length; index++) {
    if (args[index] === '--check') check = true;
    else if (['--binary', '--output'].includes(args[index]) && args[index + 1]) {
      const option = args[index++];
      if (option === '--binary') binary = resolve(ROOT, args[index]);
      else output = resolve(ROOT, args[index]);
    } else throw new Error(`Unknown or incomplete packaging option: ${args[index]}`);
  }
  checkTemplate();
  const generated = join(ROOT, 'target/native-packager');
  const assets = join(generated, 'icons');
  if (check) {
    for (const key of Object.keys(PLATFORMS)) {
      const [platform, arch] = key.split('-');
      const config = configuration(platform, arch, binary, output, assets);
      if (!config.formats.length || config.identifier !== 'dev.animaos.anima') throw new Error('Invalid package configuration');
    }
    console.log('Native packaging configuration and safe NSIS lifecycle checked for Windows x64, Linux x64, macOS arm64/x64.');
    return;
  }
  if (!existsSync(binary) || !statSync(binary).isFile()) {
    throw new Error(`Missing native-install release binary: ${binary}. Build anima with --features bundled-ui,native-install first.`);
  }
  const config = configuration(process.platform, process.arch, binary, output, assets);
  checkBinary(binary);
  const command = tool();
  generateAssets(assets);
  mkdirSync(output, { recursive: true });
  const configPath = join(generated, `packager-${process.platform}-${process.arch}.json`);
  writeFileSync(configPath, `${JSON.stringify(config, null, 2)}\n`);
  const result = spawnSync(command, ['--config', configPath], { cwd: ROOT, stdio: 'inherit' });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`cargo-packager exited with status ${result.status}`);
  console.log(`Native packages: ${output}`);
  for (const name of readdirSync(output)) console.log(`  ${name}`);
}

if (import.meta.main) {
  try { main(); }
  catch (error) { console.error(`package: ${error.message}`); process.exitCode = 1; }
}

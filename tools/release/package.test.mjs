import { describe, expect, test } from 'bun:test';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import * as packaging from './package.mjs';
const { configuration, generateAssets } = packaging;

const template = readFileSync(new URL('./nsis-installer.nsi', import.meta.url), 'utf8');

describe('native installer lifecycle', () => {
  test('installation and uninstall refuse a running Anima instead of terminating work', () => {
    const guard = template.match(/!macro CheckIfAppIsRunning\b([\s\S]*?)!macroend/)?.[1] ?? '';
    expect(guard).toContain('FindProcess');
    expect(guard).not.toContain('KillProcess');
    expect(guard).toContain('Close Anima');
    expect(guard).toContain('SetErrorLevel 3');
    expect(guard).toContain('Abort');
    expect(template.match(/!insertmacro CheckIfAppIsRunning/g)?.length).toBe(2);
  });

  test('uninstall removes login startup only when it belongs to this installation', () => {
    expect(template).toContain('ReadRegStr $R7 HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "AnimaOS"');
    expect(template).toContain('StrCmp $R7');
    expect(template).toContain('supervise --background');
    expect(template).toContain('DeleteRegValue HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Run" "AnimaOS"');
    expect(template).not.toContain('RmDir /r "{{unescape_dollar_sign this}}"');
  });

  test('silent installs compare the installed version before changing an installation', () => {
    const init = template.match(/Function \.onInit\b([\s\S]*?)FunctionEnd/)?.[1] ?? '';
    expect(init).toContain('Call EnforceInstalledVersion');
    expect(template).toContain('Function ReadInstalledVersion');
    expect(template).toContain('SetErrorLevel 4');
  });

  test('upgrade retains only matching login startup and retargets it after success', () => {
    const success = template.match(/Function \.onInstSuccess\b([\s\S]*?)FunctionEnd/)?.[1] ?? '';
    expect(template).toContain('Var PreviousInstallDirectory');
    expect(template).toContain('Var PreviousAutostartCommand');
    expect(success).toContain('Call RestoreUpgradeAutostart');
    const restore = template.match(/Function RestoreUpgradeAutostart\b([\s\S]*?)FunctionEnd/)?.[1] ?? '';
    expect(restore).toContain('$PreviousAutostartCommand');
    expect(restore).toContain('WriteRegStr HKCU');
    expect(restore).toContain('$INSTDIR');
  });

  test('explicit same-version maintenance uninstall exits after successful removal', () => {
    const leave = template.match(/Function PageLeaveReinstall\b([\s\S]*?)FunctionEnd/)?.[1] ?? '';
    const success = leave.slice(leave.indexOf('Delete $R1'));
    expect(success).toMatch(/\$\{If\} \$R5 == "2"\s+Quit\s+\$\{EndIf\}/);
    expect(success.indexOf('Quit')).toBeLessThan(success.indexOf('reinst_done:'));
  });

  test('uninstall removes only its matching remembered directory and an empty product key', () => {
    const uninstall = template.match(/Section Uninstall\b([\s\S]*?)SectionEnd/)?.[1] ?? '';
    expect(uninstall).toContain('ReadRegStr $R7 SHCTX "${MANUPRODUCTKEY}" ""');
    expect(uninstall).toContain('StrCmp $R7 $INSTDIR 0 keep_other_install_directory');
    expect(uninstall).toContain('DeleteRegValue SHCTX "${MANUPRODUCTKEY}" ""');
    expect(uninstall).toContain('DeleteRegKey /ifempty SHCTX "${MANUPRODUCTKEY}"');
    expect(uninstall).not.toMatch(/DeleteRegKey SHCTX "\$\{MANUPRODUCTKEY\}"/);
  });
});

describe('native package configuration', () => {
  test('publishes only a plan tag matching the workspace release version', () => {
    expect(packaging.validateReleasePlan({ announcement_tag: 'v0.1.0' }, '0.1.0')).toBe('v0.1.0');
    expect(() => packaging.validateReleasePlan({ announcement_tag: 'v9.9.9' }, '0.1.0')).toThrow('does not match');
    expect(() => packaging.validateReleasePlan({ announcement_tag: 'v0.1.0\nextra' }, '0.1.0')).toThrow('does not match');
    expect(() => packaging.validateReleasePlan({}, '0.1.0')).toThrow('does not match');
  });

  test('refuses portable or UI-less binaries and a different release version', () => {
    const valid = { version: '0.1.0', bundledUi: true, nativeInstall: true };
    expect(() => packaging.validateBuildInfo({ ...valid, nativeInstall: false }, '0.1.0')).toThrow('native-install');
    expect(() => packaging.validateBuildInfo({ ...valid, bundledUi: false }, '0.1.0')).toThrow('bundled-ui');
    expect(() => packaging.validateBuildInfo(valid, '9.9.9')).toThrow('version');
    expect(() => packaging.validateBuildInfo(valid, '0.1.0')).not.toThrow();
  });

  test('selects package formats and preserves the app identity on each supported host', () => {
    for (const [platform, arch, formats] of [
      ['win32', 'x64', ['nsis']], ['linux', 'x64', ['deb', 'appimage']],
      ['darwin', 'arm64', ['app', 'dmg']], ['darwin', 'x64', ['app', 'dmg']],
    ]) {
      const config = configuration(platform, arch, '/build/anima', '/packages', '/icons');
      expect(config.formats).toEqual(formats);
      expect(config.identifier).toBe('dev.animaos.anima');
      expect(config.binaries).toEqual([{ path: 'anima', main: true }]);
      expect(config.nsis?.appdataPaths).toBeUndefined();
    }
    const windows = configuration('win32', 'x64', '/build/anima.exe', '/packages', '/icons');
    expect(windows.nsis.installMode).toBe('currentUser');
    expect(windows.windows.allowDowngrades).toBe(false);
    const linux = configuration('linux', 'x64', '/build/anima', '/packages', '/icons');
    expect(linux.deb.depends).toContain('libc6 (>= 2.35)');
    expect(linux.deb.depends).toContain('xdg-utils');
    expect(() => configuration('linux', 'arm64', '/build/anima', '/packages', '/icons')).toThrow('not configured');
  });

  test('provides a real square PNG and six-size ICO without external image tooling', () => {
    const directory = mkdtempSync(join(tmpdir(), 'anima-package-icons-'));
    try {
      generateAssets(directory);
      const png = readFileSync(join(directory, 'anima-512.png'));
      expect(png.subarray(1, 4).toString()).toBe('PNG');
      expect(png.readUInt32BE(16)).toBe(512);
      expect(png.readUInt32BE(20)).toBe(512);
      const ico = readFileSync(join(directory, 'anima.ico'));
      expect(ico.readUInt16LE(2)).toBe(1);
      expect(ico.readUInt16LE(4)).toBe(6);
      expect(ico.readUInt32LE(18)).toBe(102);
    } finally {
      rmSync(directory, { recursive: true });
    }
  });
});

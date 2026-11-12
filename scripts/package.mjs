import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const run = (command, args) => execFileSync(command, args, { encoding: 'utf8' }).trim();
const config = JSON.parse(readFileSync(path.join(root, 'src-tauri/tauri.conf.json'), 'utf8'));
const builtApp = path.join(root, 'src-tauri/target/release/bundle/macos/System Balance.app');
const app = process.argv[2]
  ? path.resolve(process.argv[2])
  : existsSync(builtApp)
    ? builtApp
    : path.join(root, 'release/System Balance.app');
const plist = path.join(app, 'Contents/Info.plist');
const version = run('/usr/libexec/PlistBuddy', ['-c', 'Print :CFBundleShortVersionString', plist]);
if (version !== config.version)
  throw new Error(`App version ${version} differs from source ${config.version}`);
const executable = run('/usr/libexec/PlistBuddy', ['-c', 'Print :CFBundleExecutable', plist]);
const architectures = run('/usr/bin/lipo', ['-archs', path.join(app, 'Contents/MacOS', executable)])
  .split(/\s+/)
  .sort();
if (!architectures.length || architectures.some((arch) => !['arm64', 'x86_64'].includes(arch))) {
  throw new Error(`Unsupported architectures: ${architectures.join(', ')}`);
}
run('/usr/bin/codesign', ['--verify', '--deep', '--strict', app]);
const architecture = architectures.length === 2 ? 'universal' : architectures[0];
const release = path.join(root, 'release');
mkdirSync(release, { recursive: true });
const filename = `SystemBalance-${version}-macos-${architecture}.pkg`;
const output = path.join(release, filename);
const temporary = mkdtempSync(path.join(tmpdir(), 'macspace-pkg-'));
try {
  const payload = path.join(temporary, 'root');
  const components = path.join(temporary, 'components.plist');
  run('/usr/bin/ditto', [app, path.join(payload, 'Applications/System Balance.app')]);
  run('/usr/bin/pkgbuild', ['--analyze', '--root', payload, components]);
  run('/usr/bin/plutil', ['-replace', '0.BundleIsRelocatable', '-bool', 'NO', components]);
  console.log(
    run('/usr/bin/pkgbuild', [
      '--root',
      payload,
      '--component-plist',
      components,
      '--identifier',
      `${config.identifier}.installer`,
      '--version',
      version,
      '--install-location',
      '/',
      path.join(temporary, filename),
    ]),
  );
  run('/bin/cp', [path.join(temporary, filename), output]);
  const checksum = createHash('sha256').update(readFileSync(output)).digest('hex');
  writeFileSync(`${output}.sha256`, `${checksum}  ${filename}\n`);
  console.log(`Created ${output}\nSHA256 ${checksum}`);
} finally {
  rmSync(temporary, { recursive: true, force: true });
}

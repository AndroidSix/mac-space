import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const localCargo = path.join(root, '.tools/cargo');
const localRustup = path.join(root, '.tools/rustup');
const env = { ...process.env };
if (existsSync(path.join(localCargo, 'bin/cargo'))) {
  env.CARGO_HOME = localCargo;
  env.RUSTUP_HOME = localRustup;
  env.PATH = `${localCargo}/bin:${env.PATH}`;
}
const args = process.argv.slice(2);
const nativeTest = args[0] === 'test-native';
const command = nativeTest ? 'cargo' : path.join(root, 'node_modules/.bin/tauri');
const child = spawn(command, nativeTest ? ['test', '--lib', ...args.slice(1)] : args, {
  cwd: nativeTest ? path.join(root, 'src-tauri') : root,
  env,
  stdio: 'inherit',
});
child.on('error', (error) => {
  console.error(error.message);
  process.exitCode = 1;
});
child.on('exit', (code) => {
  process.exitCode = code ?? 1;
});

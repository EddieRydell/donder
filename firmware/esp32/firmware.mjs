import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, statSync, readFileSync, mkdirSync, renameSync, copyFileSync, writeFileSync } from 'node:fs';
import { delimiter, join, resolve } from 'node:path';

const directory = import.meta.dirname;
const root = resolve(directory, '../..');

function requiredPath(name, directoryOnly = false) {
  const value = process.env[name];
  if (!value || !existsSync(value) || (directoryOnly && !statSync(value).isDirectory())) {
    throw new Error(`Set ${name} to an existing ${directoryOnly ? 'directory' : 'library file or directory'}. See firmware/esp32/README.md.`);
  }
  return resolve(value);
}

function main() {
  const [mode, ...args] = process.argv.slice(2);
  if ((mode !== 'build' && mode !== 'cargo') || (mode === 'build' && args.length > 0) || (mode === 'cargo' && args.length === 0)) {
    throw new Error('Usage: pnpm firmware:build | pnpm firmware:cargo <cargo arguments>');
  }
  const library = requiredPath('DONDER_ESP_LIBCLANG_PATH');
  const compilerBin = requiredPath('DONDER_ESP_TOOLCHAIN_BIN', true);
  const compiler = join(compilerBin, process.platform === 'win32' ? 'xtensa-esp32-elf-gcc.exe' : 'xtensa-esp32-elf-gcc');
  if (!existsSync(compiler)) throw new Error(`ESP32 compiler not found: ${compiler}`);
  const env = { ...process.env };
  // Windows environment names are case-insensitive; avoid competing Path/PATH keys.
  const pathKey = Object.keys(env).find((key) => process.platform === 'win32' ? key.toUpperCase() === 'PATH' : key === 'PATH');
  const inheritedPath = pathKey === undefined ? '' : env[pathKey];
  if (pathKey !== undefined) delete env[pathKey];
  env.PATH = compilerBin + delimiter + inheritedPath;
  env.LIBCLANG_PATH = library;
  env.CARGO_TARGET_DIR = join(directory, 'target');

  function run(command, arguments_) {
    const result = spawnSync(command, arguments_, { cwd: directory, env, stdio: 'inherit' });
    if (result.error) throw result.error;
    if (result.status !== 0) throw new Error(`${command} failed (${result.signal ?? result.status}).`);
  }

  if (mode === 'cargo') {
    run('cargo', ['+esp', ...args]);
    return;
  }
  // Check packaging tooling before starting a potentially lengthy build.
  run('espflash', ['--version']);
  run('cargo', ['+esp', 'build', '--release', '--bin', 'loader', '--features', 'i2s-output', '--locked']);
  const output = join(root, 'target/firmware');
  mkdirSync(output, { recursive: true });
  const pending = join(output, 'donder-esp32.build.bin');
  const final = join(output, 'donder-esp32.bin');
  run('espflash', ['save-image', '--skip-update-check', '--chip', 'esp32', '--flash-size', '4mb', '--flash-mode', 'dio', '--flash-freq', '40mhz', '--xtal-freq', '40mhz', '--partition-table', 'partitions.csv', '--target-app-partition', 'factory', '--merge', '--skip-padding', 'target/xtensa-esp32-none-elf/release/loader', pending]);

  // This repository's partition table uses plain, unquoted CSV fields.
  const partitions = readFileSync(join(directory, 'partitions.csv'), 'utf8')
    .split(/\r?\n/).filter((line) => line.trim() && !line.trimStart().startsWith('#'))
    .map((line) => line.split(',').map((field) => field.trim()));
  const data = partitions.filter(([name]) => name === 'donder');
  if (data.length !== 1 || !/^0x[\da-f]+$/i.test(data[0][3])) throw new Error('Expected exactly one Donder data partition with a hexadecimal offset.');
  const boundary = Number(data[0][3]);
  const image = readFileSync(pending);
  if (!Number.isSafeInteger(boundary) || image.length <= 0x10000 || image.length > boundary) {
    throw new Error('Packaged image is empty or overlaps the Donder data partition.');
  }
  renameSync(pending, final);
  const bundled = join(root, 'apps/desktop/assets/firmware');
  mkdirSync(bundled, { recursive: true });
  copyFileSync(final, join(bundled, 'donder-esp32.bin'));
  const hash = createHash('sha256').update(image).digest('hex').toUpperCase();
  writeFileSync(join(bundled, 'donder-esp32.sha256'), hash + '\n', 'ascii');
  console.log(`Controller image: ${final}\nBytes: ${image.length}\nSHA256: ${hash}\nDesktop firmware assets: ${bundled}`);
}

try {
  main();
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}

import { spawnSync } from 'node:child_process';
import { existsSync, statSync, mkdirSync } from 'node:fs';
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

// The runtime's `iram` feature links these modules into instruction RAM; an
// out-of-line copy left in flash (an unannotated function or closure, or one
// inlined into flash code under a new name) roughly doubles evaluation time.
// Whole-frame evaluation (`evaluation::frames`) runs once per frame and stays in flash.
const INSTRUCTION_RAM_MODULES = /^<?donder_runtime(\[[0-9a-f]+\])?::(dsl::vm::strip|evaluation)::(?!frames::)/;
const INSTRUCTION_RAM = [0x40080000, 0x400a0000];

function checkInstructionRamPlacement(nm) {
  const elf = join(directory, 'target/xtensa-esp32-none-elf/release/loader');
  const listed = spawnSync(nm, ['--defined-only', '--demangle', elf], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  if (listed.error) throw listed.error;
  if (listed.status !== 0) throw new Error(`nm failed (${listed.status}).`);
  let placed = 0;
  const misplaced = [];
  for (const line of listed.stdout.split(/\r?\n/)) {
    const match = line.match(/^([0-9a-f]+) [tT] (.+)$/);
    if (!match || !INSTRUCTION_RAM_MODULES.test(match[2])) continue;
    const address = Number.parseInt(match[1], 16);
    if (address >= INSTRUCTION_RAM[0] && address < INSTRUCTION_RAM[1]) placed += 1;
    else misplaced.push(match[2].trim());
  }
  if (misplaced.length > 0) throw new Error(`Interpreter code linked outside instruction RAM:\n${misplaced.join('\n')}`);
  if (placed === 0) throw new Error('No interpreter code found in instruction RAM; update INSTRUCTION_RAM_MODULES.');
  console.log(`Instruction RAM: ${placed} interpreter functions placed.`);
}

// The runtime is 32-bit: 64-bit integer and double-precision arithmetic are
// software routines on the ESP32. Clippy bans the types in its source; this
// catches what the compiler lowers to them anyway, in any runtime function.
const RUNTIME_MODULES = /^<?donder_runtime(_types)?(\[[0-9a-f]+\])?::/;
const WIDE_ROUTINES = new RegExp(`<(__(?:${[
  'muldi3', 'divdi3', 'udivdi3', 'moddi3', 'umoddi3', 'ashldi3', 'ashrdi3', 'lshrdi3',
  'cmpdi2', 'ucmpdi2', 'negdi2', 'fixsfdi', 'fixunssfdi', 'floatdisf', 'floatundisf',
  'adddf3', 'subdf3', 'muldf3', 'divdf3', 'extendsfdf2', 'truncdfsf2', 'fixdfsi', 'fixunsdfsi',
  'fixdfdi', 'fixunsdfdi', 'floatsidf', 'floatunsidf', 'floatdidf', 'floatundidf',
  'eqdf2', 'nedf2', 'ltdf2', 'ledf2', 'gtdf2', 'gedf2', 'unorddf2',
].join('|')}))>`);

function checkRuntimeWidth(objdump) {
  const elf = join(directory, 'target/xtensa-esp32-none-elf/release/loader');
  const listed = spawnSync(objdump, ['--disassemble', '--demangle', '--no-show-raw-insn', elf], { encoding: 'utf8', maxBuffer: 512 * 1024 * 1024 });
  if (listed.error) throw listed.error;
  if (listed.status !== 0) throw new Error(`objdump failed (${listed.status}).`);
  let current = null;
  const wide = new Set();
  for (const line of listed.stdout.split(/\r?\n/)) {
    const header = line.match(/^[0-9a-f]+ <(.+)>:$/);
    if (header) {
      current = RUNTIME_MODULES.test(header[1]) ? header[1] : null;
      continue;
    }
    if (current === null) continue;
    const call = line.match(WIDE_ROUTINES);
    if (call) wide.add(`${current} calls ${call[1]}`);
  }
  if (wide.size > 0) throw new Error(`Runtime code uses 64-bit or double-precision arithmetic:\n${[...wide].join('\n')}`);
  console.log('Runtime width: no 64-bit or double-precision routines.');
}

function main() {
  const [mode, ...args] = process.argv.slice(2);
  const digQuad = mode === 'build' && args.length === 2 && args[0] === '--board' && args[1] === 'dig-quad';
  if ((mode !== 'build' && mode !== 'cargo') || (mode === 'build' && args.length > 0 && !digQuad) || (mode === 'cargo' && args.length === 0)) {
    throw new Error('Usage: pnpm firmware:build [--board dig-quad] | pnpm firmware:cargo <cargo arguments>');
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
  run('cargo', ['+esp', 'build', '--release', '--bin', 'loader', ...(digQuad ? ['--features', 'dig-quad'] : []), '--locked']);
  checkInstructionRamPlacement(join(compilerBin, process.platform === 'win32' ? 'xtensa-esp32-elf-nm.exe' : 'xtensa-esp32-elf-nm'));
  checkRuntimeWidth(join(compilerBin, process.platform === 'win32' ? 'xtensa-esp32-elf-objdump.exe' : 'xtensa-esp32-elf-objdump'));
  const output = join(root, 'target/firmware');
  mkdirSync(output, { recursive: true });
  const image = join(output, 'donder-esp32.bin');
  run('espflash', ['save-image', '--skip-update-check', '--chip', 'esp32', '--flash-size', '4mb', '--flash-mode', 'dio', '--flash-freq', '40mhz', '--xtal-freq', '40mhz', '--partition-table', 'partitions.csv', '--target-app-partition', 'factory', '--merge', '--skip-padding', 'target/xtensa-esp32-none-elf/release/loader', image]);
  console.log(`Controller firmware: ${image}`);
}

try {
  main();
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}

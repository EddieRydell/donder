# Donder browser API

`donder-browser` is the WebAssembly boundary for a client-side Donder demo. It
constructs an in-memory typed project, compiles effect and operator DSL source,
prepares sequence data with `donder-elaboration`, and evaluates frames with
`donder-runtime`.

Sequence edit commands are defined in `donder-sequence-api`. The desktop's
Tauri bindings and the standalone `bindings.ts` in that crate are both
generated from those Rust types with `pnpm generate:bindings`; browser clients
should import their `SequenceGuiEdit` type from the standalone file.

The current API supports one demo text fixture and an effect timeline. The
website supplies one normalized `[x, y]` position per displayed character; the
position order is the pixel order returned by `render`. Updating the text's
character count or measured positions updates fixture geometry and prepares a
new playback revision.

```ts
import init, {
  BrowserSession,
  compileEffectSource,
  compileOperatorSource,
} from "./pkg/donder_browser.js";

await init();

const session = new BrowserSession(
  characterPositions.length,
  30,
  12,
  `effect Glow { color sample() {
    return rgb(progress(), 0.1, 1.0 - progress());
  } }`,
);

session.setCharacterPositions(characterPositions);
const frame = session.render(1.25);
console.log(frame.pixels, frame.revision);

const compileResult = compileEffectSource(effectSource);
if (compileResult.diagnostics.length > 0) {
  console.error(compileResult.diagnostics);
}

const addedClip = session.addEffectSource(effectSource, 2, 4);
session.setEffectWindow(addedClip.id, 3, 5);
session.deleteEffect(addedClip.id);
```

Compile the WASM module with:

```sh
cargo build -p donder-browser --target wasm32-unknown-unknown --release
wasm-bindgen target/wasm32-unknown-unknown/release/donder_browser.wasm \
  --target web --out-dir target/donder-browser/pkg
```

The generated package belongs in the website build pipeline, not in the
repository's committed source tree. Automation clips, editable layers and
composition graphs, effect parameter controls, and full multi-fixture stage
authoring are not yet exposed by this initial bridge.

import assert from "node:assert/strict";
import { after, test } from "node:test";
import { fileURLToPath, URL } from "node:url";
import { createServer } from "vite";

const server = await createServer({
  configFile: false,
  root: fileURLToPath(new URL("..", import.meta.url)),
  server: { middlewareMode: true, ws: false },
  optimizeDeps: { noDiscovery: true }
});
after(() => server.close());
const { SequenceAudioSync, sequenceAudioKey } = await server.ssrLoadModule("/src/sequenceAudioSync.ts");
const target = (key, revision = 1) => ({ key, request: { path: key, projectRevision: revision, view: "sequence", objectKey: "main" } });

test("same-named silent sequences in different projects have separate transport identities", () => {
  assert.notEqual(sequenceAudioKey(1, "main.donder", "main", null, 60), sequenceAudioKey(2, "main.donder", "main", null, 60));
  assert.notEqual(sequenceAudioKey(1, "main.donder", "main", null, 60), sequenceAudioKey(1, "main.donder", "main", null, 90));
});

test("rapid navigation and cleanup leave the newest sequence loaded without overlapping commands", async () => {
  let release;
  const firstPending = new Promise((resolve) => { release = resolve; });
  let entered;
  const firstEntered = new Promise((resolve) => { entered = resolve; });
  let nativeAudio = null;
  let active = 0;
  const calls = [];
  const sync = new SequenceAudioSync(async (request) => {
    assert.equal(++active, 1);
    calls.push(request?.path ?? null);
    if (request?.path === "A") { entered(); await firstPending; }
    nativeAudio = request?.path ?? null;
    --active;
    return true;
  });
  const first = sync.synchronize(target("A"));
  await firstEntered;
  const oldCleanup = sync.synchronize(null);
  const obsolete = sync.synchronize(target("B"));
  const latest = sync.synchronize(target("C"));
  release();
  await Promise.all([first, oldCleanup, obsolete, latest]);
  assert.equal(nativeAudio, "C");
  assert.deepEqual(calls, ["A", "C"]);
  await sync.synchronize(target("C", 2));
  assert.deepEqual(calls, ["A", "C"], "unrelated edits must not restart playback");
  await sync.synchronize(null);
  assert.equal(nativeAudio, null);
});

test("rejected or stale loads remain eligible for the next request, and cleanup still unloads", async () => {
  let result = "stale";
  let nativeAudio = "previous";
  const sync = new SequenceAudioSync(async (request) => {
    if (request === null) { nativeAudio = null; return true; }
    if (result === "stale") return false;
    if (result === "error") throw new Error("decode failed");
    nativeAudio = request.path;
    return true;
  });
  await sync.synchronize(target("A"));
  await sync.synchronize(null);
  assert.equal(nativeAudio, null);
  result = "error";
  await assert.rejects(sync.synchronize(target("A")), /decode failed/);
  result = "accepted";
  await sync.synchronize(target("A", 2));
  assert.equal(nativeAudio, "A");
  await sync.synchronize(target("different-project/A", 3));
  assert.equal(nativeAudio, "different-project/A");
});

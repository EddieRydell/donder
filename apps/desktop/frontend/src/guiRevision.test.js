import assert from "node:assert/strict";
import { installThemeDom } from "./testSupport/themeDom.js";
import { after, beforeEach, test } from "node:test";
import { fileURLToPath, URL } from "node:url";
import { createServer } from "vite";

installThemeDom();

// Load the real store without opening a port or invoking the native backend.
const server = await createServer({
  configFile: false,
  root: fileURLToPath(new URL("..", import.meta.url)),
  server: { middlewareMode: true, ws: false },
  optimizeDeps: { noDiscovery: true }
});
after(() => server.close());
const { useAppStore, runGuiEditCommand, selectGuiObject } = await server.ssrLoadModule("/src/store.ts");
const initialState = useAppStore.getState();
const document = { type: "sequence", path: "main.donder", objectKey: "main" };
const snapshot = (stateRevision, projectRevision, overrides = {}) => ({
  stateRevision, projectRevision, projectEpoch: 1, projectRoot: "/project",
  projectHealth: "ready", activeFile: "main.donder", activeBuffer: null,
  settings: { editorViewMode: "gui" },
  activeDocumentDescriptor: {
    availableViews: ["text", "sequence"],
    defaultObjectKeys: [{ view: "sequence", objectKey: "main" }]
  },
  audioTransport: { generation: 0 }, liveOutput: { generation: 0 },
  ...overrides
});
beforeEach(() => {
  useAppStore.setState(initialState, true);
  useAppStore.getState().setSnapshot(snapshot(1, 1));
  useAppStore.getState().setGuiDocument(document);
});

test("event before edit response retains the editor and accepts the current projection", async () => {
  const request = useAppStore.getState().guiRequest;
  const edited = { ...document, durationSeconds: 30 };
  useAppStore.getState().setSnapshot(snapshot(3, 2), "event");
  assert.equal(useAppStore.getState().guiDocument, document);
  assert.equal(useAppStore.getState().guiDocumentRevision, 1);
  await assert.rejects(runGuiEditCommand(() => assert.fail("stale GUI must not edit")), /still loading/);
  assert.equal(useAppStore.getState().applyGuiEditResult(request, { snapshot: snapshot(2, 2), change: { type: "document", document: edited } }), true);
  assert.equal(useAppStore.getState().snapshot.stateRevision, 3);
  assert.equal(useAppStore.getState().guiDocument, edited);
  assert.equal(useAppStore.getState().guiDocumentRevision, 2);
  assert.equal(useAppStore.getState().guiResetRevision, 0);
});

test("edit response before events keeps the projection and request stable", () => {
  const request = useAppStore.getState().guiRequest;
  const edited = { ...document, durationSeconds: 30 };
  assert.equal(useAppStore.getState().applyGuiEditResult(request, { snapshot: snapshot(2, 2), change: { type: "document", document: edited } }), true);
  const acceptedRequest = useAppStore.getState().guiRequest;
  for (const revision of [1, 2, 3]) useAppStore.getState().setSnapshot(snapshot(revision, 2), "event");
  assert.equal(useAppStore.getState().guiRequest, acceptedRequest);
  assert.equal(useAppStore.getState().guiDocument, edited);
  assert.equal(useAppStore.getState().guiDocumentRevision, 2);
});

test("a late edit response cannot replace a newer source revision", () => {
  const request = useAppStore.getState().guiRequest;
  useAppStore.getState().setSnapshot(snapshot(4, 3), "event");
  const newer = { ...document, durationSeconds: 40 };
  useAppStore.getState().setGuiDocument(newer);
  assert.equal(useAppStore.getState().applyGuiEditResult(request, { snapshot: snapshot(2, 2), change: { type: "document", document } }), false);
  assert.equal(useAppStore.getState().guiDocument, newer);
  assert.equal(useAppStore.getState().guiDocumentRevision, 3);
});

test("a delayed edit keeps its displayed content and blocks a second mutation", async () => {
  let finish;
  const origin = useAppStore.getState().guiRequest;
  const pending = runGuiEditCommand(() => new Promise((resolve) => { finish = resolve; }), origin);
  assert.equal(useAppStore.getState().guiEditPending, true);
  assert.equal(useAppStore.getState().guiDocument, document);
  await assert.rejects(runGuiEditCommand(() => assert.fail("second mutation dispatched")), /already pending/);
  finish({ snapshot: snapshot(2, 2), change: { type: "document", document: { ...document } } });
  await pending;
  assert.equal(useAppStore.getState().guiEditPending, false);
});

test("a gesture cannot be rebased onto a projection received after pointer-down", async () => {
  const origin = useAppStore.getState().guiRequest;
  useAppStore.getState().setSnapshot(snapshot(2, 2));
  useAppStore.getState().setGuiDocument({ ...document });
  await assert.rejects(runGuiEditCommand(() => assert.fail("rebased gesture dispatched"), origin), /changed during the gesture/);
});

test("a failed mutation releases the interaction guard without clearing the frame", async () => {
  await assert.rejects(runGuiEditCommand(() => Promise.reject(new Error("write failed"))), /write failed/);
  assert.equal(useAppStore.getState().guiEditPending, false);
  assert.equal(useAppStore.getState().guiDocument, document);
});

test("undo and redo snapshots refresh the projection without resetting the editor", () => {
  for (const revision of [2, 3]) {
    useAppStore.getState().setSnapshot(snapshot(revision, revision));
    assert.equal(useAppStore.getState().guiDocument, document);
    assert.equal(useAppStore.getState().guiResetRevision, 0);
    useAppStore.getState().setGuiDocument(document);
    assert.equal(useAppStore.getState().guiDocumentRevision, revision);
  }
});

for (const [name, overrides] of [
  ["invalid project", { projectHealth: "invalid" }],
  ["analysis in progress", { projectHealth: "checking" }],
  ["Text mode", { settings: { editorViewMode: "text" } }],
  ["another file", { activeFile: "other.donder" }],
  ["another project", { projectRoot: "/other", projectEpoch: 2 }],
  ["reopening the same project", { projectEpoch: 2 }]
]) {
  test(`${name} discards the old projection and rejects its outstanding response`, () => {
    const request = useAppStore.getState().guiRequest;
    useAppStore.getState().setSnapshot(snapshot(3, 3, overrides));
    assert.equal(useAppStore.getState().guiDocument, null);
    assert.equal(useAppStore.getState().guiDocumentRevision, null);
    assert.equal(useAppStore.getState().applyGuiEditResult(request, { snapshot: snapshot(2, 2), change: { type: "document", document } }), false);
  });
}


test("owned fixtures in one source object have distinct edit identities", () => {
  const descriptor = { availableViews: ["text", "layout"], defaultObjectKeys: [{ view: "layout", objectKey: "main" }] };
  useAppStore.getState().setSnapshot(snapshot(2, 1, { activeDocumentDescriptor: descriptor }));
  useAppStore.getState().setGuiDocument({ type: "layout", document: { path: "main.donder" } });
  selectGuiObject({ path: "main.donder", objectKey: "main", view: "fixture", ownedPath: [{ type: "fixture", id: 1 }] }, "modal");
  const first = useAppStore.getState().guiRequest;
  const firstDocument = { type: "fixture", document: { name: "First" } };
  useAppStore.getState().setGuiDocument(firstDocument);
  useAppStore.getState().setSnapshot(snapshot(3, 2, { activeDocumentDescriptor: descriptor }));
  assert.deepEqual(useAppStore.getState().guiRequest.ownedPath, first.ownedPath);
  assert.equal(useAppStore.getState().guiRequest.view, "fixture");
  useAppStore.getState().setGuiDocument(firstDocument);
  selectGuiObject({ path: "main.donder", objectKey: "main", view: "fixture", ownedPath: [{ type: "fixture", id: 2 }] });
  assert.equal(useAppStore.getState().applyGuiEditResult(first, { snapshot: snapshot(3, 2, { activeDocumentDescriptor: descriptor }), change: { type: "document", document: firstDocument } }), false);
  assert.deepEqual(useAppStore.getState().guiRequest.ownedPath, [{ type: "fixture", id: 2 }]);
});


test("owned objects do not share selection or persisted viewport identities", async () => {
  const { guiObjectKey, sameGuiObject, objectViewKey } = await server.ssrLoadModule("/src/workspace/guiIdentity.ts");
  const first = { moduleId: "module", path: "show.donder", objectKey: "show", kind: "sequence", id: "show", ownedPath: [{ type: "sequence", id: 4 }] };
  const second = { ...first, ownedPath: [{ type: "sequence", id: 8 }] };
  assert.notEqual(guiObjectKey(first), guiObjectKey(second));
  assert.notEqual(objectViewKey(first), objectViewKey(second));
  assert.equal(sameGuiObject(first, second), false);
  assert.equal(sameGuiObject(first, { ...first, ownedPath: [{ id: 4, type: "sequence" }] }), true);
  assert.equal(objectViewKey(first), 'show.donder::["show",[{"type":"sequence","id":4}]]');
});

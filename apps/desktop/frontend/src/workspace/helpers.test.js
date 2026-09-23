import assert from "node:assert/strict";
import test from "node:test";

import {
  buildSemanticTree,
  locationRange,
  matchesCommand,
  rankQuickOpenFiles,
  remapWorkspacePath,
  sameWorkspacePath
} from "./helpers.ts";

const entry = (path, kind, role = "file") => ({
  path,
  kind,
  name: path.split("/").pop(),
  parent: path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "",
  role,
  ownership: "project",
  operations: ["open"],
  operationExplanation: null
});

test("semantic tree sorts directories first with natural case-insensitive ordering", () => {
  const tree = buildSemanticTree([
    entry("file10.donder", "file"),
    entry("Folder", "directory", "directory"),
    entry("file2.donder", "file"),
    entry("Folder/z.donder", "file")
  ], [], "C:/project");
  assert.deepEqual(tree.map((node) => node.name), ["Folder", "file2.donder", "file10.donder"]);
  assert.equal(tree[0].children[0].path, "Folder/z.donder");
});

test("diagnostic paths resolve from absolute project paths", () => {
  const tree = buildSemanticTree(
    [entry("sequences/show.sequence.donder", "file", "sequence")],
    [{
      path: "C:\\project\\sequences\\show.sequence.donder",
      range: null,
      severity: "error",
      code: "test",
      message: "broken"
    }],
    "C:\\project"
  );
  assert.equal(tree[0].errorCount, 1);
  assert.equal(sameWorkspacePath("C:\\project\\a.donder", "a.donder", "C:\\project"), true);
});

test("quick open ranks open files then recent files then the remaining project files", () => {
  const snapshot = {
    tabs: [{ path: "open.donder" }],
    workspaceExplorer: { recentFiles: ["recent.donder", "open.donder"] },
    projectEntries: [
      entry("other.donder", "file"),
      entry("open.donder", "file"),
      entry("recent.donder", "file")
    ]
  };
  assert.deepEqual(rankQuickOpenFiles(snapshot), ["open.donder", "recent.donder", "other.donder"]);
});

test("command filtering, path remapping, and navigation ranges are deterministic", () => {
  assert.equal(matchesCommand("Focus Problems", "View", ["errors", "sidebar"], "view error"), true);
  assert.equal(matchesCommand("Focus Problems", "View", ["errors"], "package"), false);
  assert.equal(remapWorkspacePath("effects/a.donder", "effects", "library/effects"), "library/effects/a.donder");
  assert.deepEqual(locationRange(3, 4, 5), {
    start: { line: 3, character: 4 },
    end: { line: 3, character: 9 }
  });
});

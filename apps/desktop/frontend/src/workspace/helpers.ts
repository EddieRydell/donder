import type {
  AppSnapshot,
  ProjectDiagnostic,
  TextRange,
  WorkspaceEntry
} from "../types";

export type WorkspaceTreeNode = WorkspaceEntry & {
  children?: WorkspaceTreeNode[];
  errorCount: number;
  warningCount: number;
};

const naturalCollator = new Intl.Collator(undefined, {
  numeric: true,
  sensitivity: "base"
});

export function buildSemanticTree(
  entries: WorkspaceEntry[],
  diagnostics: ProjectDiagnostic[],
  projectRoot: string | null
): WorkspaceTreeNode[] {
  const nodes = new Map<string, WorkspaceTreeNode>();
  for (const entry of entries) {
    const matching = diagnostics.filter((diagnostic) =>
      sameWorkspacePath(diagnostic.path, entry.path, projectRoot)
    );
    const node: WorkspaceTreeNode = {
      ...entry,
      errorCount: matching.filter((diagnostic) => diagnostic.severity === "error").length,
      warningCount: matching.filter((diagnostic) => diagnostic.severity === "warning").length
    };
    if (entry.kind === "directory") node.children = [];
    nodes.set(entry.path, node);
  }
  const roots: WorkspaceTreeNode[] = [];
  for (const entry of entries) {
    const node = nodes.get(entry.path);
    if (node === undefined) continue;
    const parent = nodes.get(entry.parent);
    if (entry.parent !== "" && parent !== undefined) parent.children?.push(node);
    else roots.push(node);
  }
  sortNodes(roots);
  return roots;
}

export function rankQuickOpenFiles(snapshot: AppSnapshot): string[] {
  const files = snapshot.projectEntries
    .filter((entry) => entry.kind === "file")
    .map((entry) => entry.path);
  return unique([
    ...snapshot.tabs.map((tab) => tab.path),
    ...snapshot.workspaceExplorer.recentFiles,
    ...files
  ]);
}

export function matchesCommand(
  label: string,
  category: string,
  keywords: string[],
  query: string
): boolean {
  const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const haystack = `${category} ${label} ${keywords.join(" ")}`.toLowerCase();
  return terms.every((term) => haystack.includes(term));
}

export function remapWorkspacePath(path: string, source: string, destination: string): string {
  if (path === source) return destination;
  if (path.startsWith(`${source}/`)) return `${destination}${path.slice(source.length)}`;
  return path;
}

export function sameWorkspacePath(left: string, right: string, projectRoot: string | null): boolean {
  const normalizedLeft = normalizePath(left);
  const normalizedRight = normalizePath(right);
  if (normalizedLeft === normalizedRight) return true;
  if (projectRoot === null || isAbsolutePath(right)) return false;
  return normalizedLeft === normalizePath(`${projectRoot}/${right}`);
}

export function locationRange(line: number, column: number, length = 0): TextRange {
  return {
    start: { line, character: column },
    end: { line, character: column + length }
  };
}

function sortNodes(nodes: WorkspaceTreeNode[]) {
  nodes.sort((left, right) => {
    if (left.kind !== right.kind) return left.kind === "directory" ? -1 : 1;
    return naturalCollator.compare(left.name, right.name);
  });
  for (const node of nodes) {
    if (node.children !== undefined) sortNodes(node.children);
  }
}

function unique(values: string[]): string[] {
  const seen = new Set<string>();
  return values.filter((value) => {
    if (seen.has(value)) return false;
    seen.add(value);
    return true;
  });
}

function normalizePath(path: string): string {
  return path.replace(/^\/\/\?\//, "").replace(/\\/g, "/").toLowerCase();
}

function isAbsolutePath(path: string): boolean {
  const normalized = normalizePath(path);
  return /^[a-z]:\//.test(normalized) || normalized.startsWith("/");
}

/**
 * The project health to display. Each edit re-checks the project; while a project that had errors
 * is re-checked, its errors stay current until the check replaces them, so it keeps showing as
 * invalid instead of flickering between checking and invalid on every keystroke.
 */
export function displayedProjectHealth(snapshot: Pick<AppSnapshot, "projectHealth" | "diagnostics">): AppSnapshot["projectHealth"] {
  if (snapshot.projectHealth === "checking" && snapshot.diagnostics.some((diagnostic) => diagnostic.severity === "error")) return "invalid";
  return snapshot.projectHealth;
}

import type {
  AppSnapshot,
  IoDiagnostic,
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
  diagnostics: IoDiagnostic[],
  projectRoot: string | null
): WorkspaceTreeNode[] {
  const nodes = new Map<string, WorkspaceTreeNode>();
  for (const entry of entries) {
    const matching = diagnostics.filter((diagnostic) =>
      sameWorkspacePath(diagnostic.path, entry.path, projectRoot)
    );
    const node: WorkspaceTreeNode = {
      ...entry,
      ...severityCounts(matching)
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

/** How many of `diagnostics` are errors and how many warnings. */
export function severityCounts(diagnostics: readonly IoDiagnostic[]): { errorCount: number; warningCount: number } {
  const errorCount = diagnostics.filter((diagnostic) => diagnostic.severity === "error").length;
  return { errorCount, warningCount: diagnostics.length - errorCount };
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

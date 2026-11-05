import { AlertTriangle, Files, Search } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { commands } from "../api";
import { FOCUS_SIDEBAR_EVENT } from "../commandRegistry";
import { runSnapshotCommand, useAppStore, type AppStaticSnapshot } from "../store";
import type {
  CommandId,
  SidebarView,
  WorkspaceLayoutState,
  WorkspacePathChangePlan
} from "../types";
import { ExplorerView } from "./ExplorerView";
import { ProblemsView } from "./ProblemsView";
import { SearchView } from "./SearchView";
import { Button, Dialog, IconButton } from "../ui/kit";

const activityItems: Array<{
  view: SidebarView;
  label: string;
  icon: typeof Files;
  command: CommandId;
}> = [
  { view: "explorer", label: "Explorer", icon: Files, command: "view.focusExplorer" },
  { view: "search", label: "Search", icon: Search, command: "view.focusSearch" },
  { view: "problems", label: "Problems", icon: AlertTriangle, command: "view.focusProblems" }
];

export function WorkspaceSidebar({
  snapshot,
  layout,
  onLayoutChange
}: {
  snapshot: AppStaticSnapshot;
  layout: WorkspaceLayoutState;
  onLayoutChange: (layout: WorkspaceLayoutState, immediate?: boolean) => void;
}) {
  const [pendingPlan, setPendingPlan] = useState<WorkspacePathChangePlan | null>(null);
  const setError = useAppStore((state) => state.setError);
  const focus = useCallback((view: SidebarView) => {
    onLayoutChange({ ...layout, activeSidebarView: view, sidebarCollapsed: false }, true);
  }, [layout, onLayoutChange]);

  useEffect(() => {
    const onFocus = (event: Event) => { focus((event as CustomEvent<SidebarView>).detail); };
    window.addEventListener(FOCUS_SIDEBAR_EVENT, onFocus);
    return () => { window.removeEventListener(FOCUS_SIDEBAR_EVENT, onFocus); };
  }, [focus]);

  const requestPathChange = async (source: string, destination: string) => {
    try {
      const request = { source, destination, projectRevision: snapshot.projectRevision };
      const plan = await commands.planWorkspacePathChange(request);
      setError(null);
      if (plan.structural) setPendingPlan(plan);
      else await runSnapshotCommand(() => commands.applyWorkspacePathChange(request));
    } catch (error) {
      setError(String(error));
      throw error;
    }
  };

  return (
    <>
      <nav className="activity-bar" aria-label="Workbench views">
        {activityItems.map(({ view, label, icon, command }) => (
          <IconButton
            key={view}
            icon={icon}
            label={label}
            command={command}
            pressed={layout.activeSidebarView === view && !layout.sidebarCollapsed}
            badge={view === "problems" && snapshot.diagnostics.length > 0
              ? <span className="activity-badge">{snapshot.diagnostics.length}</span>
              : undefined}
            onClick={() => {
              if (layout.activeSidebarView === view && !layout.sidebarCollapsed) {
                onLayoutChange({ ...layout, sidebarCollapsed: true }, true);
              } else {
                focus(view);
              }
            }}
          />
        ))}
      </nav>
      {!layout.sidebarCollapsed && (
        <aside className="workspace-sidebar">
          {layout.activeSidebarView === "explorer" && (
            <ExplorerView snapshot={snapshot} onRequestPathChange={requestPathChange} />
          )}
          {layout.activeSidebarView === "search" && <SearchView snapshot={snapshot} />}
          {layout.activeSidebarView === "problems" && <ProblemsView snapshot={snapshot} />}
        </aside>
      )}
      <PathRefactorDialog
        plan={pendingPlan}
        onCancel={() => { setPendingPlan(null); }}
        onConfirm={() => {
          if (pendingPlan === null) return;
          const request = pendingPlan.request;
          setPendingPlan(null);
          void runSnapshotCommand(() => commands.applyWorkspacePathChange(request));
        }}
      />
    </>
  );
}

function PathRefactorDialog({
  plan,
  onCancel,
  onConfirm
}: {
  plan: WorkspacePathChangePlan | null;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const groups = plan === null
    ? []
    : [
        ["Documents", plan.impact.documents],
        ["Importing documents", plan.impact.imports],
        ["Assets", plan.impact.assets],
        ["Open files", plan.impact.openFiles],
        ["Recent files", plan.impact.recentFiles],
        ["Persisted editor state", plan.impact.persistedState]
      ] as const;
  return (
    <Dialog
      open={plan !== null}
      onOpenChange={(open) => { if (!open) onCancel(); }}
      title="Move and update references"
      size="lg"
      actions={<>
        <Button onClick={onCancel}>Cancel</Button>
        <Button variant="primary" autoFocus onClick={onConfirm}>Move</Button>
      </>}
    >
      <p><strong>{plan?.request.source}</strong> → <strong>{plan?.request.destination}</strong></p>
      <p className="dialog-warning">Clears undo history.</p>
      <div className="path-impact-groups">
        {groups.filter(([, values]) => values.length > 0).map(([label, values]) => (
          <section key={label}>
            <h3>{label} <span>{values.length}</span></h3>
            <ul>{values.map((value) => <li key={value}>{value}</li>)}</ul>
          </section>
        ))}
      </div>
    </Dialog>
  );
}

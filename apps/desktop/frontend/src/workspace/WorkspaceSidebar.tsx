import * as Dialog from "@radix-ui/react-dialog";
import * as Tooltip from "@radix-ui/react-tooltip";
import { AlertTriangle, Boxes, Files, Search, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { commands } from "../api";
import { FOCUS_SIDEBAR_EVENT } from "../commandRegistry";
import { runSnapshotCommand, useAppStore, type AppStaticSnapshot } from "../store";
import { THEME_METRICS } from "../theme";
import type {
  SidebarView,
  WorkspaceLayoutState,
  WorkspacePathChangePlan
} from "../types";
import { ExplorerView } from "./ExplorerView";
import { PackagesView } from "./PackagesView";
import { ProblemsView } from "./ProblemsView";
import { SearchView } from "./SearchView";

const activityItems: Array<{
  view: SidebarView;
  label: string;
  icon: typeof Files;
}> = [
  { view: "explorer", label: "Explorer", icon: Files },
  { view: "search", label: "Search", icon: Search },
  { view: "packages", label: "Packages", icon: Boxes },
  { view: "problems", label: "Problems", icon: AlertTriangle }
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
    <Tooltip.Provider delayDuration={THEME_METRICS.tooltipDelayMs}>
      <nav className="activity-bar" aria-label="Workbench views">
        {activityItems.map(({ view, label, icon: Icon }) => (
          <Tooltip.Root key={view}>
            <Tooltip.Trigger asChild>
              <button
                type="button"
                className={layout.activeSidebarView === view && !layout.sidebarCollapsed ? "active" : ""}
                aria-label={label}
                aria-pressed={layout.activeSidebarView === view && !layout.sidebarCollapsed}
                onClick={() => {
                  if (layout.activeSidebarView === view && !layout.sidebarCollapsed) {
                    onLayoutChange({ ...layout, sidebarCollapsed: true }, true);
                  } else {
                    focus(view);
                  }
                }}
              >
                <Icon size={THEME_METRICS.activityIconSize} />
                {view === "problems" && snapshot.diagnostics.length > 0 && (
                  <span className="activity-badge">{snapshot.diagnostics.length}</span>
                )}
              </button>
            </Tooltip.Trigger>
            <Tooltip.Portal>
              <Tooltip.Content className="tooltip-content" side="right">{label}</Tooltip.Content>
            </Tooltip.Portal>
          </Tooltip.Root>
        ))}
      </nav>
      {!layout.sidebarCollapsed && (
        <aside className="workspace-sidebar">
          {layout.activeSidebarView === "explorer" && (
            <ExplorerView snapshot={snapshot} onRequestPathChange={requestPathChange} />
          )}
          {layout.activeSidebarView === "search" && <SearchView snapshot={snapshot} />}
          {layout.activeSidebarView === "packages" && <PackagesView snapshot={snapshot} />}
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
    </Tooltip.Provider>
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
        ["Manifests and lockfiles", plan.impact.manifests],
        ["Assets", plan.impact.assets],
        ["Local modules", plan.impact.modules],
        ["Open files", plan.impact.openFiles],
        ["Recent files", plan.impact.recentFiles],
        ["Persisted editor state", plan.impact.persistedState]
      ] as const;
  return (
    <Dialog.Root open={plan !== null} onOpenChange={(open) => { if (!open) onCancel(); }}>
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content className="dialog-content path-refactor-dialog">
          <Dialog.Title>Confirm structural path change</Dialog.Title>
          <Dialog.Description>
            Donder will move <strong>{plan?.request.source}</strong> to <strong>{plan?.request.destination}</strong> and rewrite typed references atomically.
          </Dialog.Description>
          <p className="dialog-warning">
            GUI undo/redo and pending rewrite state will be cleared after this operation.
          </p>
          <div className="path-impact-groups">
            {groups.filter(([, values]) => values.length > 0).map(([label, values]) => (
              <section key={label}>
                <h3>{label} <span>{values.length}</span></h3>
                <ul>{values.map((value) => <li key={value}>{value}</li>)}</ul>
              </section>
            ))}
          </div>
          <div className="dialog-actions">
            <Dialog.Close asChild><button type="button"><X size={THEME_METRICS.iconSizeSmall} /> Cancel</button></Dialog.Close>
            <button type="button" onClick={onConfirm}>Apply path change</button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

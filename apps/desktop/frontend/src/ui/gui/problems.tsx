import { createContext, useContext, useEffect, useLayoutEffect, useState, type ReactNode } from "react";
import { commands } from "../../api";
import { useAppStore } from "../../store";
import type { GuiDocumentRequest, GuiEditCommand, GuiField, GuiMember, GuiProblem, GuiProblemSite } from "../../types";

/** The open object itself, as a problem's member. */
export const OBJECT: GuiMember = { type: "object" };

/** Whether two parts of a problem's site are the same: same kind, same id, key or name. */
function samePart(left: GuiMember | GuiField, right: GuiMember | GuiField): boolean {
  const a = left as Record<string, unknown>;
  const b = right as Record<string, unknown>;
  const keys = Object.keys(a);
  return keys.length === Object.keys(b).length && keys.every((key) => a[key] === b[key]);
}

/** Whether `site` is `field` of one of `members`; a null field is the member as a whole. */
function at(site: GuiProblemSite, members: GuiMember[], field: GuiField | null): boolean {
  return members.some((member) => samePart(site.member, member))
    && (site.field === null || field === null ? site.field === field : samePart(site.field, field));
}

/** A draft form's checked problem; inside one, fields show it instead of a refused edit's. */
const DraftProblemContext = createContext<GuiProblem | null | undefined>(undefined);

/** Fields inside show `problem`, the draft's checked problem, or none when it checks. */
export function DraftProblem({ problem, children }: { problem: GuiProblem | null; children: ReactNode }) {
  return <DraftProblemContext.Provider value={problem}>{children}</DraftProblemContext.Provider>;
}

/**
 * The message of the problem at `field` of `member`, or of any of `member` when the field edits
 * several together: a draft form's checked problem, else the last refused edit's. A field that
 * shows a refused edit claims it, so the status bar shows only what no field shows.
 */
export function useFieldProblem(member: GuiMember | GuiMember[], field: GuiField | null): string | null {
  const draft = useContext(DraftProblemContext);
  const refused = useAppStore((store) => store.refusedEdit);
  const members = Array.isArray(member) ? member : [member];
  const shown = (problem: GuiProblem | null | undefined) => problem !== null && problem !== undefined && problem.site !== null && at(problem.site, members, field) ? problem.message : null;
  const fromDraft = shown(draft);
  const fromRefusal = fromDraft === null ? shown(refused) : null;
  const claims = fromRefusal !== null;
  useLayoutEffect(() => {
    if (!claims) return;
    useAppStore.setState((store) => ({ refusedEditClaims: store.refusedEditClaims + 1 }));
    return () => { useAppStore.setState((store) => ({ refusedEditClaims: store.refusedEditClaims - 1 })); };
  }, [claims, refused]);
  return fromDraft ?? fromRefusal;
}

/** The last refused edit's message while no field shows it: the status bar's to say. */
export function useUnplacedRefusal(): string | null {
  return useAppStore((store) => store.refusedEdit !== null && store.refusedEditClaims === 0 ? store.refusedEdit.message : null);
}

/**
 * What would refuse a draft form's `edit` of the object `origin` opens, checked after each change:
 * `checking` until the latest edit is checked. Pass a stable `edit`; a new one is checked again.
 */
export function useDraftCheck(origin: GuiDocumentRequest | null, edit: GuiEditCommand | null): { checking: boolean; problem: GuiProblem | null } {
  const [checked, setChecked] = useState<{ edit: GuiEditCommand; problem: GuiProblem | null } | null>(null);
  useEffect(() => {
    if (origin === null || edit === null) return;
    let current = true;
    void commands.checkGuiEdit(origin, edit).then((problem) => {
      if (current) setChecked({ edit, problem });
    });
    return () => { current = false; };
  }, [origin, edit]);
  if (edit === null) return { checking: false, problem: null };
  return checked?.edit === edit ? { checking: false, problem: checked.problem } : { checking: true, problem: null };
}

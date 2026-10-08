import type { GuiLayoutFixture } from "../../../types";

/** Layout items by id. Groups list members that may belong to several groups. */
export type LayoutItems = ReadonlyMap<number, GuiLayoutFixture>;

export const layoutIndex = (fixtures: GuiLayoutFixture[]): LayoutItems => new Map(fixtures.map((fixture) => [fixture.id, fixture]));

export const membersOf = (item: GuiLayoutFixture): number[] => item.kind.type === "group" ? item.kind.members : [];

/** `id` and every item reachable through its members, each once, depth-first. */
export function descendants(items: LayoutItems, id: number): number[] {
  const seen = new Set<number>();
  const visit = (current: number) => {
    if (seen.has(current)) return;
    seen.add(current);
    const item = items.get(current);
    if (item !== undefined) membersOf(item).forEach(visit);
  };
  visit(id);
  return [...seen];
}

/** The placed fixtures an item denotes; a fixture reached twice keeps its first position. */
export const memberFixtures = (items: LayoutItems, id: number): number[] => descendants(items, id).filter((member) => items.get(member)?.kind.type === "fixture");

/** How many groups (and the root) list an item. */
export const membershipCount = (fixtures: GuiLayoutFixture[], root: number[], id: number): number =>
  (root.includes(id) ? 1 : 0) + fixtures.filter((fixture) => membersOf(fixture).includes(id)).length;

export const nextLayoutId = (fixtures: GuiLayoutFixture[]): number => Math.max(0, ...fixtures.map((fixture) => fixture.id)) + 1;

/** Append a new item under a group, or the root for `null`. */
export function withMember(fixtures: GuiLayoutFixture[], root: number[], parent: number | null, item: GuiLayoutFixture): { fixtures: GuiLayoutFixture[]; root: number[] } {
  const added = [...fixtures, item];
  if (parent === null) return { fixtures: added, root: [...root, item.id] };
  return {
    fixtures: added.map((fixture) => fixture.id === parent && fixture.kind.type === "group" ? { ...fixture, kind: { type: "group", members: [...fixture.kind.members, item.id] } } : fixture),
    root
  };
}

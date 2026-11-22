import type { GuiGroupMember, GuiLayoutFixture } from "../../../types";

/** Layout items by id. Groups list members that may belong to several groups. */
export type LayoutItems = ReadonlyMap<number, GuiLayoutFixture>;

export type GroupSlice = Extract<GuiGroupMember, { type: "slice" }>;

export const layoutIndex = (fixtures: GuiLayoutFixture[]): LayoutItems => new Map(fixtures.map((fixture) => [fixture.id, fixture]));

export const groupMembers = (item: GuiLayoutFixture): GuiGroupMember[] => item.kind.type === "group" ? item.kind.members : [];

/** The items a group lists whole, without its slices. */
export const membersOf = (item: GuiLayoutFixture): number[] => groupMembers(item).flatMap((member) => member.type === "item" ? [member.id] : []);

/** The item a member names: the item, or the sliced fixture. */
export const memberItem = (member: GuiGroupMember): number => member.type === "item" ? member.id : member.fixture;

/** `id` and every item reachable through its members, sliced fixtures included, each once, depth-first. */
export function descendants(items: LayoutItems, id: number): number[] {
  const seen = new Set<number>();
  const visit = (current: number) => {
    if (seen.has(current)) return;
    seen.add(current);
    const item = items.get(current);
    if (item !== undefined) groupMembers(item).map(memberItem).forEach(visit);
  };
  visit(id);
  return [...seen];
}

/** The placed fixtures an item denotes; a fixture reached twice keeps its first position. */
export const memberFixtures = (items: LayoutItems, id: number): number[] => descendants(items, id).filter((member) => items.get(member)?.kind.type === "fixture");

/** How many pixels an item denotes: whole fixtures and slices, each reached once. */
export function memberPixelCount(items: LayoutItems, id: number, pixelCounts: ReadonlyMap<number, number>): number {
  const seen = new Set<string>();
  let total = 0;
  const visit = (member: GuiGroupMember) => {
    const key = member.type === "item" ? `item:${member.id}` : `slice:${member.fixture}:${member.start}:${member.count}`;
    if (seen.has(key)) return;
    seen.add(key);
    if (member.type === "slice") { total += member.count; return; }
    const item = items.get(member.id);
    if (item === undefined) return;
    if (item.kind.type === "fixture") total += pixelCounts.get(item.id) ?? 0;
    else groupMembers(item).forEach(visit);
  };
  visit({ type: "item", id });
  return total;
}

/** How many groups (and the root) list an item whole. */
export const membershipCount = (fixtures: GuiLayoutFixture[], root: number[], id: number): number =>
  (root.includes(id) ? 1 : 0) + fixtures.filter((fixture) => membersOf(fixture).includes(id)).length;

export const nextLayoutId = (fixtures: GuiLayoutFixture[]): number => Math.max(0, ...fixtures.map((fixture) => fixture.id)) + 1;

/** Append a new item under a group, or the root for `null`. */
export function withMember(fixtures: GuiLayoutFixture[], root: number[], parent: number | null, item: GuiLayoutFixture): { fixtures: GuiLayoutFixture[]; root: number[] } {
  const added = [...fixtures, item];
  if (parent === null) return { fixtures: added, root: [...root, item.id] };
  return {
    fixtures: added.map((fixture) => fixture.id === parent && fixture.kind.type === "group" ? { ...fixture, kind: { type: "group", members: [...fixture.kind.members, { type: "item", id: item.id }] } } : fixture),
    root
  };
}

/** Each placed fixture's pixel count, from the render plan's pixels. */
export function ownedPixelCounts(pixels: { owner: number }[]): Map<number, number> {
  const counts = new Map<number, number>();
  for (const pixel of pixels) counts.set(pixel.owner, (counts.get(pixel.owner) ?? 0) + 1);
  return counts;
}

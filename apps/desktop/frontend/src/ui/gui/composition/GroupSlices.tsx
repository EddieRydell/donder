import { X } from "lucide-react";
import type { GuiGroupMember, GuiLayoutFixture } from "../../../types";
import { AddRow, Field, IconButton, NumberField, Section, Select } from "../../kit";
import { groupMembers } from "./layoutGraph";

/** A group's slices: runs of a placed fixture's pixels it lists, edited in place. */
export function GroupSlices({ group, fixtures, pixelCounts, editable, onChange }: {
  group: GuiLayoutFixture;
  fixtures: GuiLayoutFixture[];
  pixelCounts: ReadonlyMap<number, number>;
  editable: boolean;
  onChange: (members: GuiGroupMember[]) => void;
}) {
  const members = groupMembers(group);
  const placed = fixtures.filter((fixture) => fixture.kind.type === "fixture");
  const count = (fixture: number) => pixelCounts.get(fixture) ?? 0;
  const replace = (index: number, member: GuiGroupMember | null) => {
    onChange(member === null ? members.filter((_, at) => at !== index) : members.map((current, at) => at === index ? member : current));
  };
  const first = placed[0];
  return <Section title="Slices" defaultOpen={members.some((member) => member.type === "slice")}>
    {members.map((member, index) => member.type !== "slice" ? null : <div className="setup-patch-node" key={`${index}:${member.fixture}`}>
      <div className="setup-patch-fields">
        <Field label="Fixture">
          <Select name="Slice fixture" value={String(member.fixture)} disabled={!editable}
            options={placed.map((fixture) => ({ value: String(fixture.id), label: `${fixture.name} (${count(fixture.id)} px)` }))}
            onChange={(fixture) => { replace(index, { type: "slice", fixture: Number(fixture), start: 0, count: Math.max(1, count(Number(fixture))) }); }} />
        </Field>
        <Field label="First pixel">
          <NumberField name="Slice first pixel" value={member.start + 1} step={1} min={1} max={Math.max(1, count(member.fixture))} integer disabled={!editable}
            onCommit={(start) => { replace(index, { ...member, start: start - 1, count: Math.max(1, Math.min(member.count, count(member.fixture) - start + 1)) }); }} />
        </Field>
        <Field label="Pixel count">
          <NumberField name="Slice pixel count" value={member.count} step={1} min={1} max={Math.max(1, count(member.fixture) - member.start)} integer disabled={!editable}
            onCommit={(pixels) => { replace(index, { ...member, count: pixels }); }} />
        </Field>
      </div>
      <IconButton icon={X} label="Remove slice" danger disabled={!editable} onClick={() => { replace(index, null); }} />
    </div>)}
    <AddRow label="Add slice" disabled={!editable || first === undefined}
      onAdd={() => { if (first !== undefined) onChange([...members, { type: "slice", fixture: first.id, start: 0, count: Math.max(1, count(first.id)) }]); }} />
  </Section>;
}

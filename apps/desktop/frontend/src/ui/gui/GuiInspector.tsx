import type { InspectedGuiDocument } from "./shared";
import { ConsoleInspector } from "./console/ConsoleInspector";
import { SequenceInspector } from "./sequence/SequenceInspector";

export function GuiInspector({ gui }: { gui: InspectedGuiDocument }) {
  return gui.type === "console" ? <ConsoleInspector document={gui.document} /> : <SequenceInspector document={gui.document} />;
}

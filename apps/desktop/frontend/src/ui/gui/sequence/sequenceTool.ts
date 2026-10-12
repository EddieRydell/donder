import { create } from "zustand";

/** The timeline's pointer tool: select and drag clips, or slice them in two. */
export type SequenceTool = "select" | "slice";

export const useSequenceTool = create<{ tool: SequenceTool; setTool: (tool: SequenceTool) => void }>((set) => ({
  tool: "select",
  setTool: (tool) => { set({ tool }); }
}));

export function toggleSliceTool() {
  const { tool, setTool } = useSequenceTool.getState();
  setTool(tool === "slice" ? "select" : "slice");
}

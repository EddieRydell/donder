import { commands as generatedCommands } from "./generated/bindings";

type CommandResult<T> = { status: "ok"; data: T } | { status: "error"; error: unknown };
type GeneratedCommands = typeof generatedCommands;
type Unwrapped<R> = R extends Promise<CommandResult<infer T>> ? Promise<T> : R;
type Commands = {
  [K in keyof GeneratedCommands]: (...args: Parameters<GeneratedCommands[K]>) => Unwrapped<ReturnType<GeneratedCommands[K]>>;
};

/** The generated commands, with every tauri-specta `Result` resolved to its data or rejected as an `Error`. */
export const commands = Object.fromEntries(
  Object.entries(generatedCommands).map(([name, command]) => [
    name,
    async (...args: unknown[]) => unwrapResult(await (command as (...args: unknown[]) => Promise<unknown>)(...args))
  ])
) as Commands;

function unwrapResult(value: unknown): unknown {
  if (!isCommandResult(value)) return value;
  if (value.status === "error") throw new Error(String(value.error));
  return value.data;
}

function isCommandResult(value: unknown): value is CommandResult<unknown> {
  if (typeof value !== "object" || value === null || Object.keys(value).length !== 2 || !("status" in value)) return false;
  return (value.status === "ok" && "data" in value) || (value.status === "error" && "error" in value);
}

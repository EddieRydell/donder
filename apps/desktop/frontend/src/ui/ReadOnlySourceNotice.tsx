import { commandRegistry } from "../commandRegistry";

export function ReadOnlySourceNotice({ name }: { name: string }) {
  return <div className="resource-editor-readonly">
    <span><strong>Read-only package source: {name}.</strong> To change package definitions independently, create a separate project with local copies of all imported sources.</span>
    <button type="button" onClick={() => void commandRegistry["file.copyProject"].run()}>Create standalone project copy...</button>
  </div>;
}

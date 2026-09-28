export function ReadOnlySourceNotice({ name }: { name: string }) {
  return <div className="resource-editor-readonly">
    <span><strong>Source unavailable: {name}.</strong> Reopen a local document to edit it.</span>
  </div>;
}

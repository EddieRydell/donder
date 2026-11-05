import { sameGuiObject } from "../../../workspace/guiIdentity";
import { X } from "lucide-react";
import { useState } from "react";
import { commands } from "../../../api";
import { runGuiEditCommand, useAppStore } from "../../../store";
import type { GuiDocumentRequest, GuiPixelRoute, PatchGuiDocument } from "../../../types";
import { AddRow, Button, Field, IconButton, NumberField, Select, Toggle } from "../../kit";
import { ReferenceInput } from "../setup/PatchInputs";

type Draft = { origin: GuiDocumentRequest; routes: GuiPixelRoute[] };

const RGB_ORDER: [number, number, number] = [1, 0, 2];
const RGBW_ORDER: [number, number, number, number] = [1, 0, 2, 3];
const ENCODINGS = [{ value: "rgb", label: "RGB" }, { value: "rgbw", label: "RGBW" }] as const;
const COMPONENTS = ["Red", "Green", "Blue", "White"];

export function PatchEditor({ document }: { document: PatchGuiDocument }) {
  const [draft, setDraft] = useState<Draft | null>(null);
  const [error, setError] = useState<string | null>(null);
  const request = useAppStore((state) => state.guiRequest);
  const revision = useAppStore((state) => state.guiDocumentRevision);
  const pending = useAppStore((state) => state.guiEditPending);
  const stale = draft !== null && draft.origin !== request;
  const routable = document.controllers.length > 0 && document.layouts.some((layout) => layout.fixtures.some((fixture) => fixture.pixelCount > 0));
  const changeRoute = (id: number, route: GuiPixelRoute) => {
    if (draft !== null) setDraft({ ...draft, routes: draft.routes.map((item) => item.id === id ? route : item) });
  };
  const addRoute = () => {
    if (draft === null) return;
    const layout = document.layouts.find((layout) => layout.fixtures.some((fixture) => fixture.pixelCount > 0));
    const fixture = layout?.fixtures.find((fixture) => fixture.pixelCount > 0);
    const controller = document.controllers[0];
    const port = controller?.ports[0];
    if (layout === undefined || fixture === undefined || controller === undefined || port === undefined) return;
    setDraft({ ...draft, routes: [...draft.routes, {
      id: Math.max(0, ...draft.routes.map((route) => route.id)) + 1,
      layout: layout.sourceRef, fixture: fixture.id, pixels: null,
      controller: controller.sourceRef, port: port.id, startSlot: 0,
      encoding: { type: "rgb", order: [...RGB_ORDER] }, gamma: 1, brightness: 1
    }] });
  };
  const disabled = pending || stale;
  return <main className="setup-editor">
    <header className="object-overview-header"><h2>LED output routes</h2><span>{document.path}</span></header>
    {draft === null ? <section className="setup-section">
      {document.routes.map((route) => <p key={route.id}>
        {document.layouts.find((layout) => sameGuiObject(layout.sourceRef, route.layout))?.fixtures.find((fixture) => fixture.id === route.fixture)?.name}
        {" → "}{document.controllers.find((controller) => sameGuiObject(controller.sourceRef, route.controller))?.label ?? "Missing controller"}, port {route.port}, channel {route.startSlot + 1}
        {" · "}{route.encoding.type.toUpperCase()}
      </p>)}
      <Button disabled={pending || request === null || revision !== request.projectRevision}
        onClick={() => { if (request !== null) { setDraft({ origin: request, routes: structuredClone(document.routes) }); setError(null); } }}>Edit routes</Button>
    </section> : <form className="setup-section" onSubmit={(event) => {
      event.preventDefault();
      void runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "patch", routes: draft.routes }), draft.origin)
        .then(() => { setDraft(null); setError(null); }).catch((error: unknown) => { setError(String(error)); });
    }}>
      {stale && <p role="alert" className="kit-status-error">Out of date</p>}
      {error !== null && <p role="alert" className="kit-status-error">{error}</p>}
      {draft.routes.map((route) => <section className="setup-patch-node" key={route.id}>
        <RouteFields document={document} route={route} disabled={disabled} onChange={(next) => { changeRoute(route.id, next); }} />
        <IconButton icon={X} label="Remove route" danger disabled={disabled} onClick={() => { setDraft({ ...draft, routes: draft.routes.filter((item) => item.id !== route.id) }); }} />
      </section>)}
      <AddRow label="Add route" disabled={disabled || !routable} onAdd={addRoute} />
      <div className="dialog-actions">
        <Button disabled={pending} onClick={() => { setDraft(null); setError(null); }}>Discard</Button>
        <Button type="submit" variant="primary" disabled={disabled}>Apply</Button>
      </div>
    </form>}
  </main>;
}

function RouteFields({ document, route, disabled, onChange }: { document: PatchGuiDocument; route: GuiPixelRoute; disabled: boolean; onChange: (route: GuiPixelRoute) => void }) {
  const layout = document.layouts.find((layout) => sameGuiObject(layout.sourceRef, route.layout));
  const target = layout?.fixtures.find((fixture) => fixture.id === route.fixture);
  const controller = document.controllers.find((controller) => sameGuiObject(controller.sourceRef, route.controller));
  const span = route.pixels;
  return <div className="setup-patch-fields">
    <ReferenceInput label="Layout" value={route.layout} disabled={disabled} choices={document.layouts.map((layout, index) => ({ reference: layout.sourceRef, label: `${layout.sourceRef.ownedPath.length > 0 ? `Layout ${index + 1}` : layout.sourceRef.objectKey} (${layout.sourceRef.path})` }))} onChange={(reference) => {
      const fixture = document.layouts.find((layout) => sameGuiObject(layout.sourceRef, reference))?.fixtures[0];
      if (fixture !== undefined) onChange({ ...route, layout: reference, fixture: fixture.id, pixels: null });
    }} />
    <Field label="Fixture">
      <Select name="Fixture" value={String(route.fixture)} disabled={disabled} onChange={(fixture) => { onChange({ ...route, fixture: Number(fixture), pixels: null }); }}
        options={(layout?.fixtures ?? []).map((fixture) => ({ value: String(fixture.id), label: `${fixture.name} (${fixture.pixelCount} px)` }))} />
    </Field>
    <Field label="Partial" tip="Route only part of the pixel wiring">
      <Toggle name="Partial" checked={span !== null} disabled={disabled} onChange={(checked) => { onChange({ ...route, pixels: checked ? { start: 0, count: target?.pixelCount ?? 0 } : null }); }} />
    </Field>
    {span !== null && <>
      <Field label="First pixel"><NumberField name="First pixel" value={span.start + 1} step={1} min={1} integer disabled={disabled} onCommit={(start) => { onChange({ ...route, pixels: { ...span, start: start - 1 } }); }} /></Field>
      <Field label="Pixel count"><NumberField name="Pixel count" value={span.count} step={1} min={1} integer disabled={disabled} onCommit={(count) => { onChange({ ...route, pixels: { ...span, count } }); }} /></Field>
    </>}
    <ReferenceInput label="Controller" value={route.controller} disabled={disabled} choices={document.controllers.map((controller) => ({ reference: controller.sourceRef, label: controller.label }))} onChange={(reference) => {
      const port = document.controllers.find((controller) => sameGuiObject(controller.sourceRef, reference))?.ports[0];
      if (port !== undefined) onChange({ ...route, controller: reference, port: port.id });
    }} />
    <Field label="Port">
      <Select name="Port" value={String(route.port)} disabled={disabled} onChange={(port) => { onChange({ ...route, port: Number(port) }); }}
        options={(controller?.ports ?? []).map((port) => ({ value: String(port.id), label: `${port.id} (${port.slotCount} ch)` }))} />
    </Field>
    <Field label="First channel"><NumberField name="First channel" value={route.startSlot + 1} step={1} min={1} integer disabled={disabled} onCommit={(value) => { onChange({ ...route, startSlot: value - 1 }); }} /></Field>
    <Field label="Encoding">
      <Select name="Encoding" value={route.encoding.type} options={ENCODINGS} disabled={disabled} onChange={(type) => {
        onChange({ ...route, encoding: type === "rgb" ? { type, order: [...RGB_ORDER] } : { type, order: [...RGBW_ORDER] } });
      }} />
    </Field>
    {route.encoding.order.map((component, index) => <Field key={index} label={`Channel ${index + 1}`}>
      <Select name={`Channel ${index + 1}`} value={String(component)} disabled={disabled} onChange={(value) => {
        const encoding = structuredClone(route.encoding);
        const next = Number(value);
        const previous = encoding.order.indexOf(next);
        encoding.order[index] = next;
        if (previous >= 0) encoding.order[previous] = component;
        onChange({ ...route, encoding });
      }} options={COMPONENTS.slice(0, route.encoding.order.length).map((name, value) => ({ value: String(value), label: name }))} />
    </Field>)}
    <Field label="Gamma"><NumberField name="Gamma" value={route.gamma} step={0.1} min={0.01} defaultValue={1} disabled={disabled} onCommit={(gamma) => { onChange({ ...route, gamma }); }} /></Field>
    <Field label="Brightness"><NumberField name="Brightness" value={route.brightness} step={0.01} min={0} max={1} defaultValue={1} disabled={disabled} onCommit={(brightness) => { onChange({ ...route, brightness }); }} /></Field>
  </div>;
}

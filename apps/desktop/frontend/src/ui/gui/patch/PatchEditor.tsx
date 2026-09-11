import { useState } from "react";
import { commands } from "../../../api";
import { runGuiEditCommand, useAppStore } from "../../../store";
import type { GuiDocumentRequest, GuiObjectRef, GuiPixelRoute, PatchGuiDocument } from "../../../types";
import { NumberField, ReferenceInput } from "../setup/PatchInputs";

type Draft = { origin: GuiDocumentRequest; routes: GuiPixelRoute[] };

export function PatchEditor({ document }: { document: PatchGuiDocument }) {
  const [draft, setDraft] = useState<Draft | null>(null);
  const [error, setError] = useState<string | null>(null);
  const request = useAppStore((state) => state.guiRequest);
  const revision = useAppStore((state) => state.guiDocumentRevision);
  const pending = useAppStore((state) => state.guiEditPending);
  const stale = draft !== null && draft.origin !== request;
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
      encoding: { type: "rgb", order: [1, 0, 2] }, gamma: 1, brightness: 1
    }] });
  };
  return <main className="setup-editor">
    <header className="object-overview-header"><h2>LED output routes</h2><span>{document.objectKey}</span></header>
    {draft === null ? <section className="setup-section">
      {document.routes.length === 0 && <p>No pixels are routed to controllers yet.</p>}
      {document.routes.map((route) => <p key={route.id}>
        {document.layouts.find((layout) => sameReference(layout.sourceRef, route.layout))?.fixtures.find((fixture) => fixture.id === route.fixture)?.name}
        {" → "}{route.controller.objectKey}, port {route.port}, channel {route.startSlot + 1}
        {" · "}{route.encoding.type.toUpperCase()}
      </p>)}
      <button type="button" disabled={pending || request === null || revision !== request.projectRevision}
        onClick={() => { if (request !== null) { setDraft({ origin: request, routes: structuredClone(document.routes) }); setError(null); } }}>Edit routes</button>
    </section> : <form className="setup-section" onSubmit={(event) => {
      event.preventDefault();
      void runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "patch", routes: draft.routes }), draft.origin)
        .then(() => { setDraft(null); setError(null); }).catch((error: unknown) => { setError(String(error)); });
    }}>
      {stale && <p role="alert">The project changed. Discard this draft and reopen it.</p>}
      {error !== null && <p role="alert">{error}</p>}
      <fieldset disabled={pending || stale}>
        {draft.routes.map((route) => <section className="setup-patch-node" key={route.id}>
          <RouteFields document={document} route={route} onChange={(next) => { changeRoute(route.id, next); }} />
          <button type="button" onClick={() => { setDraft({ ...draft, routes: draft.routes.filter((item) => item.id !== route.id) }); }}>Remove route</button>
        </section>)}
        <button type="button" disabled={document.controllers.length === 0 || !document.layouts.some((layout) => layout.fixtures.some((fixture) => fixture.pixelCount > 0))} onClick={addRoute}>Add route</button>
        <button type="submit">Apply routes</button>
      </fieldset>
      <button type="button" disabled={pending} onClick={() => { setDraft(null); setError(null); }}>Discard draft</button>
    </form>}
  </main>;
}

function sameReference(a: GuiObjectRef, b: GuiObjectRef) {
  return a.moduleId === b.moduleId && a.path === b.path && a.objectKey === b.objectKey;
}

function RouteFields({ document, route, onChange }: { document: PatchGuiDocument; route: GuiPixelRoute; onChange: (route: GuiPixelRoute) => void }) {
  const layout = document.layouts.find((layout) => sameReference(layout.sourceRef, route.layout));
  const target = layout?.fixtures.find((fixture) => fixture.id === route.fixture);
  const controller = document.controllers.find((controller) => sameReference(controller.sourceRef, route.controller));
  const span = route.pixels;
  return <div className="setup-patch-fields">
    <ReferenceInput label="Layout" value={route.layout} choices={document.layouts.map((layout) => layout.sourceRef)} onChange={(reference) => {
      const fixture = document.layouts.find((layout) => sameReference(layout.sourceRef, reference))?.fixtures[0];
      if (fixture !== undefined) onChange({ ...route, layout: reference, fixture: fixture.id, pixels: null });
    }} />
    <label>Fixture instance or group<select value={route.fixture} onChange={(event) => { onChange({ ...route, fixture: Number(event.target.value), pixels: null }); }}>
      {layout?.fixtures.map((fixture) => <option key={fixture.id} value={fixture.id}>{fixture.name} ({fixture.pixelCount} pixels)</option>)}
    </select></label>
    <label><input type="checkbox" checked={span !== null} onChange={(event) => { onChange({ ...route, pixels: event.target.checked ? { start: 0, count: target?.pixelCount ?? 0 } : null }); }} />Route only part of the pixel wiring</label>
    {span !== null && <>
      <NumberField label="First pixel" value={span.start + 1} min={1} onChange={(start) => { onChange({ ...route, pixels: { ...span, start: start - 1 } }); }} />
      <NumberField label="Pixel count" value={span.count} min={1} onChange={(count) => { onChange({ ...route, pixels: { ...span, count } }); }} />
    </>}
    <ReferenceInput label="Controller" value={route.controller} choices={document.controllers.map((controller) => controller.sourceRef)} onChange={(reference) => {
      const port = document.controllers.find((controller) => sameReference(controller.sourceRef, reference))?.ports[0];
      if (port !== undefined) onChange({ ...route, controller: reference, port: port.id });
    }} />
    <label>Output port<select value={route.port} onChange={(event) => { onChange({ ...route, port: Number(event.target.value) }); }}>
      {controller?.ports.map((port) => <option key={port.id} value={port.id}>{port.id} ({port.slotCount} channels)</option>)}
    </select></label>
    <NumberField label="First controller channel" value={route.startSlot + 1} min={1} onChange={(value) => { onChange({ ...route, startSlot: value - 1 }); }} />
    <label>Pixel encoding<select value={route.encoding.type} onChange={(event) => {
      if (event.target.value === "rgb") onChange({ ...route, encoding: { type: "rgb", order: [1, 0, 2] } });
      else if (event.target.value === "rgbw") onChange({ ...route, encoding: { type: "rgbw", order: [1, 0, 2, 3] } });
    }}><option value="rgb">RGB</option><option value="rgbw">RGBW</option></select></label>
    {route.encoding.order.map((component, index) => <label key={index}>Channel {index + 1}<select value={component} onChange={(event) => {
      const encoding = structuredClone(route.encoding);
      const next = Number(event.target.value);
      const previous = encoding.order.indexOf(next);
      encoding.order[index] = next;
      if (previous >= 0) encoding.order[previous] = component;
      onChange({ ...route, encoding });
    }}>
      {(route.encoding.type === "rgb" ? ["Red", "Green", "Blue"] : ["Red", "Green", "Blue", "White"]).map((name, component) => <option key={name} value={component}>{name}</option>)}
    </select></label>)}
    <NumberField label="Gamma" value={route.gamma} min={0.01} step="any" onChange={(gamma) => { onChange({ ...route, gamma }); }} />
    <NumberField label="Brightness" value={route.brightness} min={0} max={1} step="any" onChange={(brightness) => { onChange({ ...route, brightness }); }} />
  </div>;
}

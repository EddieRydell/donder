import { guiObjectKey, sameGuiObject } from "../../../workspace/guiIdentity";
import { ChevronLeft, X } from "lucide-react";
import { Fragment, useState } from "react";
import { useAppStore } from "../../../store";
import { put, site } from "../edits";
import type { GuiField, GuiPixelRoute, PatchGuiDocument } from "../../../types";
import { AddRow, ContextMenuArea, Field, IconButton, NumberField, Select, Tip, Toggle } from "../../kit";
import { outputTestEntries, useOutputTesting } from "../outputTest";
import { useFieldProblem } from "../problems";
import { ReferenceInput } from "../setup/PatchInputs";
import { ColorPicker } from "../../ColorPicker";

type Controller = PatchGuiDocument["controllers"][number];
type Place = PatchGuiDocument["placements"][number]["place"];

/** GRB: WS2811 and WS2812 pixels read green, red, then blue. */
const RGB_ORDER: [number, number, number] = [1, 0, 2];
const RGBW_ORDER: [number, number, number, number] = [1, 0, 2, 3];
const ENCODINGS = [{ value: "rgb", label: "RGB" }, { value: "rgbw", label: "RGBW" }, { value: "channels", label: "Channels" }] as const;
/** The most channels a pixel of a Channels output takes. */
const MAX_CHANNEL_COLORS = 4;
const WHITE = "#ffffff";
const STARTS = [{ value: "next", label: "After previous" }, { value: "channel", label: "Channel" }] as const;
const COMPONENT_LETTERS = "RGBW";

const COMPONENTS = [0, 1, 2, 3];
const distinct = (order: number[]) => new Set(order).size === order.length;
/** Every color order, as the component each channel carries. */
/** An encoding that orders red, green, blue and white across its channels. */
type OrderedEncoding = Extract<GuiPixelRoute["encoding"], { order: readonly number[] }>;
const RGB_ORDERS: OrderedEncoding[] = COMPONENTS.slice(0, 3).flatMap((a) => COMPONENTS.slice(0, 3).flatMap((b) => COMPONENTS.slice(0, 3)
  .filter((c) => distinct([a, b, c])).map((c) => ({ type: "rgb" as const, order: [a, b, c] as [number, number, number] }))));
const RGBW_ORDERS: OrderedEncoding[] = COMPONENTS.flatMap((a) => COMPONENTS.flatMap((b) => COMPONENTS.flatMap((c) => COMPONENTS
  .filter((d) => distinct([a, b, c, d])).map((d) => ({ type: "rgbw" as const, order: [a, b, c, d] as [number, number, number, number] })))));

const orderName = (order: readonly number[]) => order.map((component) => COMPONENT_LETTERS[component]).join("");
/** A route's encoding as its rows show it: the color order, or how many channels each pixel lights. */
const encodingName = (encoding: GuiPixelRoute["encoding"]) => encoding.type === "channels" ? `${encoding.colors.length} ch` : orderName(encoding.order);

/** Where a route's channels land, as people count: `compact` for a column, `full` for its tooltip. */
function placeLabels(place: Place): { compact: string; full: string } {
  switch (place.type) {
    case "universes":
      if (place.firstUniverse === place.lastUniverse) {
        return place.firstChannel === place.lastChannel
          ? { compact: `U${place.firstUniverse}:${place.firstChannel}`, full: `universe ${place.firstUniverse}, channel ${place.firstChannel}` }
          : { compact: `U${place.firstUniverse}:${place.firstChannel}–${place.lastChannel}`, full: `universe ${place.firstUniverse}, channels ${place.firstChannel}–${place.lastChannel}` };
      }
      return {
        compact: `U${place.firstUniverse}:${place.firstChannel}–U${place.lastUniverse}:${place.lastChannel}`,
        full: `universe ${place.firstUniverse} channel ${place.firstChannel} to universe ${place.lastUniverse} channel ${place.lastChannel}`
      };
    case "port":
      return place.first === place.last
        ? { compact: `${place.port}:${place.first}`, full: `${place.port}, channel ${place.first}` }
        : { compact: `${place.port}:${place.first}–${place.last}`, full: `${place.port}, channels ${place.first}–${place.last}` };
  }
}

/** A new route's port: the controller's first; a controller without ports leaves the route on none, which its check reports. */
const firstPort = (controller: Controller | undefined) => controller?.ports[0]?.id ?? 0;

const portLabel = (port: Controller["ports"][number]) => port.donder === null ? port.name : `${port.name} (${port.donder.slotCount} ch)`;

/** The routes on each controller, its ports in order and each port's routes in route order: the order their channels follow. */
function controllerGroups(document: PatchGuiDocument) {
  const missing = (route: GuiPixelRoute) => ({ route, port: "Missing" });
  const groups = document.controllers.flatMap((controller) => {
    const routes = document.routes.filter((route) => sameGuiObject(controller.sourceRef, route.controller));
    if (routes.length === 0) return [];
    const onPorts = controller.ports.flatMap((port) => routes.filter((route) => route.port === port.id).map((route) => ({ route, port: port.name })));
    const unported = routes.filter((route) => !controller.ports.some((port) => port.id === route.port)).map(missing);
    return [{ key: guiObjectKey(controller.sourceRef), label: controller.label, routes: [...onPorts, ...unported] }];
  });
  const orphans = document.routes.filter((route) => !document.controllers.some((controller) => sameGuiObject(controller.sourceRef, route.controller)));
  return orphans.length === 0 ? groups : [...groups, { key: "", label: "Missing", routes: orphans.map(missing) }];
}

/** The project's outputs: how each fixture is wired to a controller port, each field committed as it changes. */
export function PatchEditor({ document, onClose }: { document: PatchGuiDocument; onClose: () => void }) {
  /** The right-clicked route row. */
  const [menuRoute, setMenuRoute] = useState<number | null>(null);
  const testing = useOutputTesting();
  const request = useAppStore((state) => state.guiRequest);
  const revision = useAppStore((state) => state.guiDocumentRevision);
  const pending = useAppStore((state) => state.guiEditPending);
  const routable = document.controllers.length > 0 && document.fixtures.some((fixture) => fixture.pixelCount > 0);
  const putRoutes = (routes: GuiPixelRoute[]) => void put(site(null), { type: "routes", routes });
  const addRoute = () => {
    const fixture = document.fixtures.find((fixture) => fixture.pixelCount > 0);
    const controller = document.controllers[0];
    if (fixture === undefined || controller === undefined) return;
    putRoutes([...document.routes, {
      id: Math.max(0, ...document.routes.map((route) => route.id)) + 1,
      fixture: fixture.id, pixels: null, reverse: false,
      controller: controller.sourceRef, port: firstPort(controller), start: { type: "next" },
      encoding: { type: "rgb", order: [...RGB_ORDER] }, gamma: 1, brightness: 1, dither: false
    }]);
  };
  return <main className="object-editor">
    <header className="object-overview-header"><h2>Outputs</h2><IconButton icon={ChevronLeft} label="Back to the setup" onClick={onClose} /></header>
    <ContextMenuArea entries={menuRoute === null ? [] : outputTestEntries({ type: "route", route: menuRoute }, pending || request === null || revision !== request.projectRevision, testing)}><section className="object-section" onContextMenuCapture={(event) => {
      const row = event.target instanceof Element ? event.target.closest("[data-route]") : null;
      setMenuRoute(row === null ? null : Number(row.getAttribute("data-route")));
    }}>
      {controllerGroups(document).map((group) => <Fragment key={group.key}>
        <h3>{group.label}</h3>
        {group.routes.map(({ route, port }) => {
          const place = document.placements.find((placement) => placement.route === route.id)?.place;
          const fixture = document.fixtures.find((candidate) => candidate.id === route.fixture)?.name ?? "Missing";
          const labels = place === undefined ? null : placeLabels(place);
          const order = encodingName(route.encoding);
          return <Tip key={route.id} label={`${fixture} → ${group.label} port ${port}${labels === null ? "" : `, ${labels.full}`} · ${order}`}>
            <div className="patch-route-row" data-route={route.id}>
              <span>{fixture}</span><span>{port}</span><span>{labels?.compact ?? ""}</span><span>{order}</span>
            </div>
          </Tip>;
        })}
      </Fragment>)}
    </section></ContextMenuArea>
    <section className="object-section">
      {document.routes.map((route) => <section className="setup-patch-node" key={route.id}>
        <RouteFields document={document} route={route} disabled={pending} onChange={(next) => { putRoutes(document.routes.map((item) => item.id === route.id ? next : item)); }} />
        <IconButton icon={X} label="Remove output" danger disabled={pending} onClick={() => { putRoutes(document.routes.filter((item) => item.id !== route.id)); }} />
      </section>)}
      <AddRow label="Add output" disabled={pending || !routable} onAdd={addRoute} />
    </section>
  </main>;
}

function RouteFields({ document, route, disabled, onChange }: { document: PatchGuiDocument; route: GuiPixelRoute; disabled: boolean; onChange: (route: GuiPixelRoute) => void }) {
  const target = document.fixtures.find((fixture) => fixture.id === route.fixture);
  const controller = document.controllers.find((controller) => sameGuiObject(controller.sourceRef, route.controller));
  const span = route.pixels;
  const member = { type: "route", id: route.id } as const;
  const problems = {
    target: useFieldProblem(member, { type: "target" }),
    pixels: useFieldProblem(member, { type: "pixels" }),
    controller: useFieldProblem(member, { type: "controller" }),
    port: useFieldProblem(member, { type: "port" }),
    firstChannel: useFieldProblem(member, { type: "firstChannel" }),
    colorOrder: useFieldProblem(member, { type: "colorOrder" }),
    gamma: useFieldProblem(member, { type: "gamma" }),
    brightness: useFieldProblem(member, { type: "brightness" })
  } satisfies Partial<Record<GuiField["type"], string | null>>;
  /** The problem's message when it concerns `field` of this route. */
  const error = (field: keyof typeof problems) => problems[field];
  return <div className="setup-patch-fields">
    <Field label="Fixture" error={error("target")}>
      <Select name="Fixture" value={String(route.fixture)} disabled={disabled} invalid={error("target") !== null} onChange={(fixture) => { onChange({ ...route, fixture: Number(fixture), pixels: null }); }}
        options={document.fixtures.map((fixture) => ({ value: String(fixture.id), label: `${fixture.name} (${fixture.pixelCount} px)` }))} />
    </Field>
    <Field label="Partial" tip="Route only part of the pixel wiring">
      <Toggle name="Partial" checked={span !== null} disabled={disabled} onChange={(checked) => { onChange({ ...route, pixels: checked ? { start: 0, count: target?.pixelCount ?? 0 } : null }); }} />
    </Field>
    <Field label="First pixel" error={error("pixels")}>
      <NumberField name="First pixel" value={(span?.start ?? 0) + 1} step={1} min={1} integer disabled={disabled || span === null} invalid={error("pixels") !== null}
        onCommit={(start) => { if (span !== null) onChange({ ...route, pixels: { ...span, start: start - 1 } }); }} />
    </Field>
    <Field label="Pixel count" error={error("pixels")}>
      <NumberField name="Pixel count" value={span?.count ?? target?.pixelCount ?? 0} step={1} min={1} integer disabled={disabled || span === null} invalid={error("pixels") !== null}
        onCommit={(count) => { if (span !== null) onChange({ ...route, pixels: { ...span, count } }); }} />
    </Field>
    <Field label="Reverse" tip="Send the pixels last first, for wiring that runs against the fixture's pixel order">
      <Toggle name="Reverse" checked={route.reverse} disabled={disabled} onChange={(reverse) => { onChange({ ...route, reverse }); }} />
    </Field>
    <ReferenceInput label="Controller" value={route.controller} disabled={disabled} error={error("controller")} choices={document.controllers.map((controller) => ({ reference: controller.sourceRef, label: controller.label }))} onChange={(reference) => {
      const next = document.controllers.find((controller) => sameGuiObject(controller.sourceRef, reference));
      onChange({ ...route, controller: reference, port: firstPort(next) });
    }} />
    <Field label="Port" error={error("port")}>
      <Select name="Port" value={String(route.port)} disabled={disabled} invalid={error("port") !== null}
        onChange={(port) => { onChange({ ...route, port: Number(port) }); }}
        options={[
          ...(controller?.ports.some((port) => port.id === route.port) === true ? [] : [{ value: String(route.port), label: "Choose", disabled: true }]),
          ...(controller?.ports ?? []).map((port) => ({ value: String(port.id), label: portLabel(port) }))
        ]} />
    </Field>
    <Field label="Start" tip="After the output before it on its port, or at a channel">
      <Select name="Start" value={route.start.type} options={STARTS} disabled={disabled}
        onChange={(type) => { onChange({ ...route, start: type === "next" ? { type } : { type, channel: 0 } }); }} />
    </Field>
    <Field label="First channel" error={error("firstChannel")} {...(controller?.config.type === "donder" ? {} : { tip: "Counted across all the controller's universes" })}>
      <NumberField name="First channel" value={route.start.type === "channel" ? route.start.channel + 1 : 1} step={1} min={1} integer disabled={disabled || route.start.type !== "channel"} invalid={error("firstChannel") !== null}
        onCommit={(value) => { onChange({ ...route, start: { type: "channel", channel: value - 1 } }); }} />
    </Field>
    <Field label="Encoding">
      <Select name="Encoding" value={route.encoding.type} options={ENCODINGS} disabled={disabled} onChange={(type) => {
        onChange({ ...route, encoding: type === "rgb" ? { type, order: [...RGB_ORDER] } : type === "rgbw" ? { type, order: [...RGBW_ORDER] } : { type, colors: [WHITE] } });
      }} />
    </Field>
    {route.encoding.type === "channels" ? <ChannelColors colors={route.encoding.colors} disabled={disabled} error={error("colorOrder")}
      onChange={(colors) => { onChange({ ...route, encoding: { type: "channels", colors } }); }} />
      : <Field label="Color order" tip="The colors each pixel reads, in channel order; WS2811 and WS2812 pixels are GRB" error={error("colorOrder")}>
        <Select name="Color order" value={orderName(route.encoding.order)} disabled={disabled} invalid={error("colorOrder") !== null} onChange={(name) => {
          const encoding = route.encoding.type === "rgb"
            ? RGB_ORDERS.find((candidate) => orderName(candidate.order) === name)
            : RGBW_ORDERS.find((candidate) => orderName(candidate.order) === name);
          if (encoding !== undefined) onChange({ ...route, encoding });
        }} options={(route.encoding.type === "rgb" ? RGB_ORDERS : RGBW_ORDERS).map(({ order }) => ({ value: orderName(order), label: orderName(order) }))} />
      </Field>}
    <Field label="Gamma" error={error("gamma")}><NumberField name="Gamma" value={route.gamma} step={0.1} min={0.01} defaultValue={1} disabled={disabled} invalid={error("gamma") !== null} onCommit={(gamma) => { onChange({ ...route, gamma }); }} /></Field>
    <Field label="Brightness" error={error("brightness")}><NumberField name="Brightness" value={route.brightness} step={0.01} min={0} max={1} defaultValue={1} disabled={disabled} invalid={error("brightness") !== null} onCommit={(brightness) => { onChange({ ...route, brightness }); }} /></Field>
    <Field label="Dither" tip="Vary each channel over frames so dim levels keep their color; for outputs that receive every frame">
      <Toggle name="Dither" checked={route.dither} disabled={disabled} onChange={(dither) => { onChange({ ...route, dither }); }} />
    </Field>
  </div>;
}

/** The color each channel of a Channels output lights, in channel order: white for a dimmer, or the color of a single-color string. */
function ChannelColors({ colors, disabled, error, onChange }: { colors: string[]; disabled: boolean; error: string | null; onChange: (colors: string[]) => void }) {
  return <Field label="Channels" tip="The color each channel lights; white dims to the pixel's brightness, a color lights as much of it as the pixel holds" error={error}>
    <div className="patch-channel-colors">
      {colors.map((color, index) => <div className="patch-channel-color" key={index}>
        <ColorPicker label={`Channel ${index + 1}`} value={color} commit={(next) => { onChange(colors.map((item, at) => at === index ? next : item)); return Promise.resolve(); }} />
        <IconButton icon={X} label={`Remove channel ${index + 1}`} disabled={disabled || colors.length === 1} onClick={() => { onChange(colors.filter((_, at) => at !== index)); }} />
      </div>)}
      <AddRow label="Add channel" disabled={disabled || colors.length >= MAX_CHANNEL_COLORS} onAdd={() => { onChange([...colors, WHITE]); }} />
    </div>
  </Field>;
}

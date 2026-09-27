/** Canonical opaque RGB used by project colors and native color inputs. */
export function normalizeHexColor(value: string): string | null {
  const match = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(value.trim());
  const hex = match?.[1];
  if (hex === undefined) return null;
  return `#${hex.length === 3 ? hex.replace(/[0-9a-f]/gi, "$&$&") : hex}`.toLowerCase();
}

export function opaqueRgbBytes(value: string): [number, number, number] {
  const color = normalizeHexColor(value);
  if (color === null) throw new Error(`Expected an opaque RGB color, received: ${value}`);
  return [
    Number.parseInt(color.slice(1, 3), 16),
    Number.parseInt(color.slice(3, 5), 16),
    Number.parseInt(color.slice(5, 7), 16)
  ];
}

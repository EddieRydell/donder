/** Canonical opaque RGB used by project colors and native color inputs. */
export function normalizeHexColor(value: string): string | null {
  const match = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(value.trim());
  const hex = match?.[1];
  if (hex === undefined) return null;
  return `#${hex.length === 3 ? hex.replace(/[0-9a-f]/gi, "$&$&") : hex}`.toLowerCase();
}

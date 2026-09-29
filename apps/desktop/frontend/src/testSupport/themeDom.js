import { readFileSync } from "node:fs";
import { URL } from "node:url";

export function installThemeDom() {
  const stylesheet = readFileSync(new URL("../styles.css", import.meta.url), "utf8");
  const cssVariables = new Map(
    [...stylesheet.matchAll(/^\s*(--donder-[\w-]+):\s*([^;]+);/gm)].map(([, name, value]) => [name, value.trim()])
  );
  const cssValue = (name, seen = new Set()) => {
    if (seen.has(name)) throw new Error(`Circular CSS variable: ${name}`);
    const value = cssVariables.get(name) ?? "";
    const reference = /^var\((--donder-[\w-]+)\)$/.exec(value);
    return reference === null ? value : cssValue(reference[1], new Set([...seen, name]));
  };
  Object.assign(globalThis, {
    document: { documentElement: {} },
    getComputedStyle: () => ({ getPropertyValue: cssValue })
  });
}

import { cpp } from "@codemirror/lang-cpp";
import { yaml } from "@codemirror/lang-yaml";
import { HighlightStyle } from "@codemirror/language";
import type { Extension } from "@codemirror/state";
import { tags } from "@lezer/highlight";
import type { TextDocumentSyntax } from "../../editor/types";

export function languageForSyntax(syntax: TextDocumentSyntax): Extension {
  return syntax === "effectDsl" ? cpp() : yaml();
}

export const donderHighlightStyle = HighlightStyle.define([
  { tag: tags.keyword, color: "var(--donder-code-keyword)" },
  { tag: [tags.name, tags.propertyName, tags.attributeName], color: "var(--donder-code-name)" },
  { tag: [tags.variableName, tags.definition(tags.variableName)], color: "var(--donder-text)" },
  { tag: [tags.function(tags.variableName), tags.function(tags.definition(tags.variableName))], color: "var(--donder-code-function)" },
  { tag: [tags.string, tags.special(tags.string)], color: "var(--donder-code-string)" },
  { tag: [tags.number, tags.bool, tags.null], color: "var(--donder-code-number)" },
  { tag: [tags.operator, tags.punctuation, tags.separator], color: "var(--donder-text-muted)" },
  { tag: tags.comment, color: "var(--donder-text-muted)", fontStyle: "italic" },
  { tag: [tags.typeName, tags.className], color: "var(--donder-code-type)" },
  { tag: tags.invalid, color: "var(--donder-code-invalid)" }
]);

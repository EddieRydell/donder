import { yaml } from "@codemirror/lang-yaml";
import { HighlightStyle, StreamLanguage, type StringStream } from "@codemirror/language";
import type { Extension } from "@codemirror/state";
import { tags } from "@lezer/highlight";
import type { TextDocumentSyntax } from "../../editor/types";

// Mirrors the effect language lexer (donder-language dsl/syntax/lexer.rs).
const KEYWORDS = new Set(["effect", "operator", "fn", "let", "guard", "if", "else", "for", "in"]);
const MEMBERS = new Set(["param", "input", "sample"]);
const TYPES = new Set(["int", "float", "bool", "color", "curve", "gradient", "marks", "enum", "array"]);
const CONTEXT = new Set(["time", "duration", "progress", "pixel", "target", "PI", "TAU"]);
const OPERATORS = /^(\.\.=|\.\.|->|\/\/|&&|\|\||[<>=!]=|[-+*/%<>=!])/;

/** Whether `pattern` matches at the cursor, consuming it unless `peek`. */
function at(stream: StringStream, pattern: RegExp, peek = false): boolean {
  const matched = stream.match(pattern, !peek);
  return matched !== null && matched !== false;
}

const donderDsl = StreamLanguage.define<null>({
  name: "donder",
  startState: () => null,
  token(stream: StringStream): string | null {
    if (stream.eatSpace()) return null;
    if (at(stream, /^--/)) {
      stream.skipToEnd();
      return "comment";
    }
    if (at(stream, /^#[0-9a-fA-F]{6}\b/)) return "color";
    if (at(stream, /^\d+(\.\d+)?/)) return "number";
    if (at(stream, /^[A-Za-z_][A-Za-z0-9_]*/)) {
      const word = stream.current();
      if (word === "true" || word === "false") return "bool";
      if (KEYWORDS.has(word) || MEMBERS.has(word)) return "keyword";
      if (TYPES.has(word)) return "type";
      if (CONTEXT.has(word)) return "context";
      if (at(stream, /^\s*\(/, true) || at(stream, /^\s+for\b/, true)) return "function";
      return "name";
    }
    if (at(stream, OPERATORS)) return "operator";
    stream.next();
    return "punctuation";
  },
  languageData: { commentTokens: { line: "--" } },
  tokenTable: {
    comment: tags.comment,
    color: tags.special(tags.string),
    number: tags.number,
    bool: tags.bool,
    keyword: tags.keyword,
    type: tags.typeName,
    context: tags.propertyName,
    function: tags.function(tags.variableName),
    name: tags.variableName,
    operator: tags.operator,
    punctuation: tags.punctuation
  }
});

export function languageForSyntax(syntax: TextDocumentSyntax): Extension {
  return syntax === "effectDsl" ? donderDsl : yaml();
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

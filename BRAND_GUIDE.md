# Donder Brand Guide

This document governs the visual system and interaction rules for Donder.

## 1. Core Identity

**Donder is a precise technical workbench for authoring light shows as source projects.**

It should feel like a professional desktop instrument in the family of Ableton, FL Studio, Zed and Blender, not a web page or a visualizer. The interface exists to expose project structure, validation, sequencing, preview, and output control with clarity.

> The UI should stay quieter than the show being created.

### 1.1 Logo

- The mark is a gray cloud: four circles on a flat base, cloud `#646b75`.
- The cloud alone is the small logo: app icon, favicon, tray. The icon is the cloud on a `#16191d` rounded tile; its master is `apps/desktop/assets/icons/icon.svg`.
- In larger spaces the lockup sets lowercase `donder` in Nunito ExtraBold, outlined, with `do` on the cloud and a gap cut around the letters. The website's `src/components/brand-logo.tsx` holds it.
- Running text says Donder, capitalized; the lowercase form belongs to the lockup.
- Regenerate the bundled icons with `pnpm tauri icon apps/desktop/assets/icons/icon.svg -o target/icons`, then copy `icon.png`, `icon.ico` and `icon.icns` into `apps/desktop/assets/icons/`.

## 2. Brand Principles

### 2.1 Editor First
- The project graph is the source of truth.
- Text, structure, diagnostics, and previews should feel connected.
- Avoid marketing-style surfaces, decorative panels, and visual noise.

### 2.2 Controlled, Not Flashy
- Use grayscale as the default visual language.
- Use color only when the app needs to communicate data or state.

### 2.3 Dense And Direct
- Dense, predictable, keyboard-friendly, and inspectable.
- Manipulate the thing itself instead of filling in a form about it.
- Every gesture means one thing everywhere, so users learn it once without reading.

### 2.4 GUI Over Hidden State
- Every important project concept should eventually have a GUI view.
- GUI views should complement the files, not replace them.

## 3. Visual Philosophy

### 3.1 Neutral By Default
- Core surfaces are near-neutral gray. A slight tint is fine; a visibly colored gray or colored black is not.
- Large UI areas never carry brand color.

### 3.2 State Through Structure
Use contrast, borders, weight, and placement before color.
- Active file: raised neutral surface.
- Hover: slightly lighter neutral surface.
- Playing: a neutral filled button, not a colored button.
- Status bar: neutral dark surface, not a brand strip.

## 4. Color System

Every static value is a `--donder-*` custom property in `apps/desktop/frontend/src/styles.css` and reaches TypeScript through `theme.ts`.

### 4.1 Neutral Tokens

| Token | Purpose |
| --- | --- |
| `--donder-bg` | Main app background |
| `--donder-bg-rail` | Tool windows and sidebars |
| `--donder-bg-panel` | Top bars, popovers, controls |
| `--donder-hover` | Hover and selected surfaces |
| `--donder-border` | Standard 1px borders |
| `--donder-border-control` | Inputs and emphasized separators |
| `--donder-text` | Primary text |
| `--donder-text-muted` | Secondary text |
| `--donder-text-faint` | Tertiary text |

Rules:
- Prefer changing a token over adding one; never add a near-duplicate.
- Keep new neutrals on the existing ramp; do not add a differently tinted gray.

### 4.2 Accent Color

There is no persistent accent. Color is reserved for data the user authored (clips, marks, curves, fixture colors), the playhead, focus, and the semantic colors below.

### 4.3 Semantic Colors
- Red (`--donder-danger`) means an actual error, or a destructive action under the pointer. Destructive controls rest neutral and turn red on hover; red is never a resting decoration.
- Warning is for diagnostics only.
- Semantic colors stay muted. A small mark of color beats a filled red surface.

## 5. Typography

- UI font: Inter (`--donder-font-ui`). Code: the monospace stack.
- Text is 10 to 14px; dialogs may use 16px for a title.
- Section labels are small uppercase with label letter spacing.
- No decorative text effects, gradient or glowing text.

## 6. Layout And Shape

- Top menu and window controls live in one integrated title bar.
- Tool windows use clear 1px boundaries, not cards.
- Radii are `--donder-radius-sm` (2px) and `--donder-radius-md` (3px); `--donder-radius-circle` is only for dots and knobs. No pills.
- Spacing uses `--donder-space-{2,4,8,12,16,20,24}`, named by pixel value and on the 4px grid.
- Control heights come from `--donder-control-height-*`; do not invent a row height.

## 7. Components

Every common pattern has exactly one component in `apps/desktop/frontend/src/ui/kit`: button and icon button, number field, text field, select and menu select (a choice from a menu tree, such as definitions in their folders), tabs (equal-width tabs that pick which view a panel shows), toggle, labelled field, section, badge, tooltip, menu and context menu, dialog, and empty-list "+" row. Screens compose them and never restyle raw `<button>`, `<input>`, `<select>` or `<label>`. A new pattern goes into the kit and this guide before it is used twice.

### 7.1 Surfaces
- Flat. Prefer borders over shadows.
- One elevation, `--donder-shadow-float`, for every floating surface: menus, popovers, tooltips and dialogs. No other shadows, gradients or bevels.

### 7.2 Buttons
- Default: neutral surface with a subtle border. Hover: lighter neutral surface. Active: stronger neutral surface or text weight.
- No colored backgrounds for ordinary commands.
- Controls keep the arrow cursor. Cursors change only to show what a drag does (resize, move, pan).
- Icon buttons name their action and shortcut in a tooltip.

### 7.3 Inputs
- Neutral surface, 1px border, visible focus ring (`--donder-focus`).
- Numbers are scrubbable fields, not bare text boxes (section 8).

### 7.4 Menus And Popovers
- Compact, left-aligned, icons only where they improve recognition, shortcuts right-aligned.
- Hover is grayscale.
- Context menus are built from the command registry, the same source as toolbars and shortcuts.

### 7.5 Dialogs
- Only for work that needs the user's whole attention: opening or creating a project, export, and irreversible confirmation.
- Everything else edits in place, in the inspector or a popover anchored to its control.
- A dialog is a flat panel with a title row and the minimum fields; defaults are filled in and names can be changed later in place.

### 7.6 Words
- A control that shows its state beats a sentence explaining it. Icons, badges, counts, handles and direct manipulation replace helper paragraphs, empty-state prose and "you can…" hints.
- Labels are one or two words. Descriptions and shortcuts live in tooltips, not under controls.
- An empty list shows a single "+" affordance.
- No toasts. Changes show where they happen: on the control, in the status bar, or on the undo button.

## 8. Interaction Rules

1. **Numbers.** Drag to scrub, Shift for fine, double-click to reset to default, click to type, Enter commits, Escape reverts.
2. **Delete.** Delete or Backspace removes the selection. Anything undoable is never confirmed; confirm only what undo cannot restore.
3. **Undo is visible.** Undo and Redo buttons in the title bar show whether a step exists and name it in their tooltip. A focused text field keeps its own history, made visible by its focus ring.
4. **Escape** backs out one level: cancel the drag, close the popover, clear the selection.
5. **Modifiers.** Shift constrains or extends. Alt bypasses snapping. Space-drag or middle-drag pans. Ctrl or Cmd with the wheel zooms; Shift with the wheel scrolls sideways.
6. **Right-click** opens a menu on every object, listing each item's shortcut.
7. **Selection.** Click selects; Shift or Ctrl adds; drag draws a marquee; Ctrl+A selects all. Same on every canvas and list.
8. **Tooltips** appear after the shared delay and show the live binding from the command registry, never a hard-coded key.
9. **Focus** is always visible, tab order follows reading order, and canvases never swallow global shortcuts.
10. **Feel.** Drags and scrubs update at display rate; pointer-move work stays out of React state.
11. Chrome text is not selectable, nothing rubber-bands, and nothing underlines on hover.

## 9. Anti-Patterns

- Web tells: pointer cursors on buttons, pills, cards, toasts, soft drop shadows, hover underlines.
- Teal, cyan, purple, or gradient accents. Neon effects, decorative glow or shadow.
- Large colored status bars or resting red surfaces.
- Cards inside cards.
- Explanatory prose where a control or icon would do.
- Color used where contrast or spacing would be enough.

## 10. One-Line Definition

> A quiet, grayscale instrument for turning light-show source projects into editable, previewable, validated systems.

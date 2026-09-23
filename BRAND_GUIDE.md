# Donder Brand Guide

This document governs the visual system for Donder.

## 1. Core Identity

**Donder is a precise technical workbench for authoring light shows as source projects.**

It should feel closer to a professional IDE than a visualizer. The interface exists to expose project structure, validation, sequencing, preview, and output control with clarity.

> The UI should stay quieter than the show being created.

## 2. Brand Principles

### 2.1 Editor First
- The project graph is the source of truth.
- Text, structure, diagnostics, and previews should feel connected.
- Avoid marketing-style surfaces, decorative panels, and visual noise.

### 2.2 Controlled, Not Flashy
- Use grayscale as the default visual language.
- Use color only when the app needs to communicate state.
- For the current prototype, avoid accent color entirely unless it is semantic and necessary.

### 2.3 Technical, But Legible
- Donder should feel like an IDE: dense, predictable, keyboard-friendly, and inspectable.
- Interactions should be direct and obvious.

### 2.4 GUI Over Hidden State
- Every important project concept should eventually have a GUI view.
- GUI views should complement the files, not replace them.

## 3. Visual Philosophy

### 3.1 Neutral By Default
- All core surfaces use neutral grayscale values where R = G = B.
- Avoid blue-gray, teal-gray, purple-gray, or colored black.
- Large UI areas should never carry brand color.

### 3.2 State Through Structure
Use contrast, borders, weight, and placement before color.

For now, active and selected states should be gray-only:
- Active file: raised neutral surface.
- Hover: slightly lighter neutral surface.
- Playing: a neutral filled button, not a colored button.
- Status bar: neutral dark surface, not a brand strip.

## 4. Color System

### 4.1 Neutral Tokens

The Donder desktop app should define colors as CSS custom properties in `apps/desktop/src/styles.css`.

| Token | Purpose |
| --- | --- |
| `--bg` | Main app background |
| `--surface-1` | Tool windows and sidebars |
| `--surface-2` | Top bars, popovers, controls |
| `--surface-3` | Hover and selected surfaces |
| `--border` | Standard 1px borders |
| `--border-strong` | Inputs and emphasized separators |
| `--text` | Primary text |
| `--text-muted` | Secondary text |
| `--text-faint` | Tertiary text |

Rules:
- Core tokens must be grayscale hex values.
- Prefer changing tokens over adding one-off colors.
- Do not introduce blue-tinted neutrals.

### 4.2 Accent Color

No persistent accent color is defined yet.

Future accent use must be:
- Sparse.
- Purposeful.
- Isolated to active controls, playhead/timeline markers, or primary actions.

### 4.3 Semantic Colors

For the prototype, diagnostics may use neutral grayscale styling instead of colored severity. When semantic colors are introduced, they should be tokenized and limited to diagnostics or destructive actions.

## 5. Typography

- UI font: system stack, matching native desktop expectations.
- No decorative text effects.
- No gradient or glowing text.
- Use compact, readable sizing for toolbars, menus, sidebars, and inspectors.

## 6. Layout And Spacing

- The app should resemble a JetBrains-style IDE workbench.
- Top menu and window controls live in one integrated title bar.
- Tool windows use clear boundaries, not decorative cards.
- Use 4-6px radii for small controls; avoid pill-shaped controls unless the component requires it.

## 7. Components

### 7.1 Surfaces
- Use grayscale tokens only.
- Prefer borders over shadows.
- Shadows are allowed for menus/popovers only.

### 7.2 Buttons
- Default: neutral surface with a subtle border.
- Hover: lighter neutral surface.
- Active: stronger neutral surface or text weight.
- Avoid colored button backgrounds for ordinary commands.

### 7.3 Inputs
- Neutral surface.
- 1px border.
- Focus can use `--border-strong` until a dedicated accent exists.

### 7.4 Menus
- Compact.
- Left-aligned text.
- Icons only where they improve recognition.
- Hover should be grayscale, not blue.

## 8. Anti-Patterns

- Blue-gray UI surfaces.
- Teal, cyan, purple, or gradient accents.
- Neon effects.
- Large colored status bars.
- Decorative glow or shadow.
- Cards inside cards.
- Color used where contrast or spacing would be enough.

## 9. One-Line Definition

> A quiet, grayscale IDE for turning light-show source projects into editable, previewable, validated systems.


import { opaqueRgbBytes } from "./color";
import { THEME_COLORS, THEME_METRICS } from "./theme";
import type { PreviewAppearance } from "./types";

export const PREVIEW_APPEARANCE: PreviewAppearance = {
  backgroundRgb: opaqueRgbBytes(THEME_COLORS.previewBackground),
  unlitRgb: opaqueRgbBytes(THEME_COLORS.previewUnlit),
  windowWidth: THEME_METRICS.previewWindowWidth,
  windowHeight: THEME_METRICS.previewWindowHeight,
  windowMinWidth: THEME_METRICS.previewWindowMinWidth,
  windowMinHeight: THEME_METRICS.previewWindowMinHeight,
  canvasFillRatio: THEME_METRICS.previewCanvasFillRatio,
  minimumRadiusPixels: THEME_METRICS.previewMinimumRadiusPixels
};

/**
 * Single source of truth for window arrangement.
 *
 * To rearrange the app, edit a template string here. Panels are
 * position-agnostic and never reference their own placement, so any panel can
 * be moved to any area without touching its code.
 */

export type Area = "search" | "sidebar" | "list" | "detail" | "similar" | "status";

export interface Layout {
  areas: string;
  columns: string;
  rows: string;
}

export const LAYOUTS = {
  default: {
    areas: `
      "search  search  search"
      "sidebar list    detail"
      "sidebar list    similar"
      "status  status  status"
    `,
    columns: "var(--sidebar-w) var(--list-w) 1fr",
    rows: "auto 1fr var(--similar-h) auto",
  },

  // Waveform across the full width, list beneath. Better on a wide monitor.
  wide: {
    areas: `
      "search  search  search"
      "sidebar detail  detail"
      "sidebar list    similar"
      "status  status  status"
    `,
    columns: "var(--sidebar-w) 1fr var(--similar-w)",
    rows: "auto var(--detail-h) 1fr auto",
  },

  // Narrow docked mode for sitting beside a script or NLE.
  compact: {
    areas: `
      "search"
      "detail"
      "list"
      "status"
    `,
    columns: "1fr",
    rows: "auto var(--detail-h) 1fr auto",
  },
} satisfies Record<string, Layout>;

export type LayoutName = keyof typeof LAYOUTS;

export const DEFAULT_LAYOUT: LayoutName = "default";

export function layoutStyle(name: LayoutName): React.CSSProperties {
  const l = LAYOUTS[name];
  return {
    display: "grid",
    gridTemplateAreas: l.areas,
    gridTemplateColumns: l.columns,
    gridTemplateRows: l.rows,
    height: "100vh",
    width: "100vw",
    overflow: "hidden",
  };
}

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
      "search  search search search search"
      "sidebar gutA   list   gutB   right"
      "status  status status status status"
    `,
    // Only the right column is flexible, so the waveform absorbs window
    // resizing while the sidebar and list keep the widths the user set.
    columns: "var(--sidebar-w) var(--gutter) var(--list-w) var(--gutter) 1fr",
    rows: "auto 1fr auto",
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

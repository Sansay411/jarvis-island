import type { CardSpec } from "./cards/types";

export type OverlayTone = "amber" | "ok" | "warn" | "danger";

export interface OverlayMonitor {
  originX: number;
  originY: number;
  width: number;
  height: number;
  scale: number;
}

export interface ScreenRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface CssRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Point2D {
  x: number;
  y: number;
}

export interface RawBox {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface OverlayRingItem {
  kind: "ring";
  id?: string;
  x: number;
  y: number;
  w: number;
  h: number;
  label?: string;
  tone?: OverlayTone;
  seconds?: number;
  persistent?: boolean;
}

export interface OverlayArrowItem {
  kind: "arrow";
  id?: string;
  to: Point2D;
  from?: Point2D;
  targetRect?: RawBox;
  tone?: OverlayTone;
  seconds?: number;
  persistent?: boolean;
}

export interface OverlayCaptionItem {
  kind: "caption";
  id?: string;
  text: string;
  x?: number;
  y?: number;
  step?: number;
  total?: number;
  badge?: string | null;
  hint?: string;
  targetRect?: RawBox;
  tone?: OverlayTone;
  seconds?: number;
  persistent?: boolean;
  closable?: boolean;
}

export interface OverlayCardItem {
  kind: "card";
  id: string;
  anchor?: RawBox;
  spec: CardSpec;
}

export type OverlayItem =
  | OverlayRingItem
  | OverlayArrowItem
  | OverlayCaptionItem
  | OverlayCardItem;

export interface OverlayDrawPayload {
  items: OverlayItem[];
  replace: boolean;
  monitor?: OverlayMonitor;
}

export interface OverlaySnapshot {
  items: OverlayItem[];
  monitor: OverlayMonitor;
}

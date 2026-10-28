import type { Model } from "./types";

const MAGIC = 0x3247574c; // "LWG2" little endian

/** Decodes the binary layout written by `src-tauri/src/gcode.rs`. */
export function decodeModel(buf: ArrayBuffer, name: string): Model {
  const head = new DataView(buf, 0, 24);
  if (head.getUint32(0, true) !== MAGIC) throw new Error("Unexpected data from the G-code parser.");
  const layerCount = head.getUint32(4, true);
  const segCount = head.getUint32(8, true);
  const bedW = head.getFloat32(12, true);
  const bedH = head.getFloat32(16, true);

  const colorCount = head.getUint32(20, true);
  const colors: string[] = [];
  for (let i = 0; i < colorCount; i++) {
    colors.push("#" + new DataView(buf).getUint32(24 + i * 4, true).toString(16).padStart(6, "0"));
  }

  let at = 24 + colorCount * 4;
  const offsets = new Uint32Array(buf, at, layerCount + 1);
  at += (layerCount + 1) * 4;
  const z = new Float32Array(buf, at, layerCount);
  at += layerCount * 4;
  const height = new Float32Array(buf, at, layerCount);
  at += layerCount * 4;
  const pos = new Float32Array(buf, at, segCount * 6);
  at += segCount * 24;
  const feature = new Uint8Array(buf, at, segCount);

  return { name, layerCount, segCount, bedW, bedH, colors, offsets, z, height, pos, feature };
}

export const FEATURES: { id: number; label: string; color: string }[] = [
  { id: 0, label: "Other", color: "#8c8a85" },
  { id: 1, label: "Outer wall", color: "#e36414" },
  { id: 2, label: "Inner wall", color: "#f2a541" },
  { id: 3, label: "Overhang wall", color: "#3b6fd1" },
  { id: 4, label: "Sparse infill", color: "#a43d5c" },
  { id: 5, label: "Solid infill", color: "#8f4bc8" },
  { id: 6, label: "Top surface", color: "#e8475c" },
  { id: 7, label: "Bottom surface", color: "#6d8f3a" },
  { id: 8, label: "Bridge", color: "#5f9ea0" },
  { id: 9, label: "Gap infill", color: "#e8e8e8" },
  { id: 10, label: "Support", color: "#2e8b57" },
  { id: 11, label: "Support interface", color: "#55c08a" },
  { id: 12, label: "Skirt / brim", color: "#4f8a8b" },
  { id: 13, label: "Prime tower", color: "#b0a060" },
];

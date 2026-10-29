export type Connection = "off" | "connecting" | "connected" | "error";

export interface Status {
  connection: Connection;
  error: string | null;
  gcodeState: string | null;
  layer: number | null;
  totalLayers: number | null;
  percent: number | null;
  remainingMin: number | null;
  jobName: string | null;
  gcodeFile: string | null;
  nozzle: number | null;
  nozzleTarget: number | null;
  bed: number | null;
  bedTarget: number | null;
  /** Non-zero while the printer reports an error. */
  printError: number | null;
  /** 1 silent, 2 standard, 3 sport, 4 ludicrous. */
  speedLevel: number | null;
  lightOn: boolean | null;
  wifi: string | null;
}

export interface Settings {
  host: string;
  accessCode: string;
  serial: string;
  /** Desktop notifications at 25/50/75%, first layer, finished, paused, failed. */
  notify: boolean;
}

export interface Model {
  name: string;
  layerCount: number;
  segCount: number;
  bedW: number;
  bedH: number;
  /** Filament color per tool (T0, T1...) as "#rrggbb"; may be empty. */
  colors: string[];
  /** First segment of each layer; length layerCount + 1. */
  offsets: Uint32Array;
  z: Float32Array;
  height: Float32Array;
  /** x1 y1 z1 x2 y2 z2 per segment. */
  pos: Float32Array;
  /** Low 4 bits: feature id (FEATURES). High 4 bits: tool index. */
  feature: Uint8Array;
}

/** `"download-progress"` event while fetching the job from the printer. */
export interface DownloadProgress {
  received: number;
  /** Null when the printer doesn't report the file size. */
  total: number | null;
}

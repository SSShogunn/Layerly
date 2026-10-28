import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { FEATURES } from "./gcode";
import type { Model } from "./types";

/** Nominal road width (mm) — A1 mini 0.4 nozzle lines are ~0.42. */
const ROAD_WIDTH = 0.42;
const PALETTE_SIZE = 16;
/** Used when the G-code names no filament color. */
const DEFAULT_FILAMENT = "#d9d4cc";
const GHOST_ALPHA = 0.09;

/**
 * One road segment as a box without a bottom face, in local coordinates:
 * u ∈ {0,1} along the segment (start → end), v ∈ {-½,½} across it,
 * w ∈ {-1,0} down from the top of the road. The vertex shader maps these
 * onto each instance. 20 vertices / 10 triangles.
 */
function roadGeometry(): THREE.BufferGeometry {
  const pos: number[] = [];
  const nrm: number[] = [];
  const idx: number[] = [];
  type V = [number, number, number];
  // Quad o, o+a, o+a+b, o+b; a × b points outward (CCW when seen from outside).
  const quad = (o: V, a: V, b: V) => {
    const n: V = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    const base = pos.length / 3;
    const corners: V[] = [
      o,
      [o[0] + a[0], o[1] + a[1], o[2] + a[2]],
      [o[0] + a[0] + b[0], o[1] + a[1] + b[1], o[2] + a[2] + b[2]],
      [o[0] + b[0], o[1] + b[1], o[2] + b[2]],
    ];
    for (const c of corners) {
      pos.push(...c);
      nrm.push(...n);
    }
    idx.push(base, base + 1, base + 2, base, base + 2, base + 3);
  };
  const U: V = [1, 0, 0];
  const Vv: V = [0, 1, 0];
  const W: V = [0, 0, 1];
  quad([0, -0.5, 0], U, Vv); // top (+w)
  quad([0, 0.5, -1], W, U); // left side (+v)
  quad([0, -0.5, -1], U, W); // right side (-v)
  quad([1, -0.5, -1], Vv, W); // end cap (+u)
  quad([0, -0.5, -1], W, Vv); // start cap (-u)

  const g = new THREE.BufferGeometry();
  g.setAttribute("position", new THREE.Float32BufferAttribute(pos, 3));
  g.setAttribute("normal", new THREE.Float32BufferAttribute(nrm, 3));
  g.setIndex(idx);
  return g;
}

const VERT = /* glsl */ `
attribute vec3 aStart;
attribute vec3 aEnd;
attribute float aFeature; // low 4 bits feature id, high 4 bits tool
attribute vec2 aInfo; // x = layer index, y = layer height (mm)

uniform float uWidth;
uniform float uLayer;
uniform float uHighlight; // 1 = tint the current layer
uniform float uColorMode; // 0 = filament colors, 1 = line types
uniform vec3 uPalette[${PALETTE_SIZE}];
uniform vec3 uFilament[${PALETTE_SIZE}];

varying vec3 vNormal;
varying vec3 vColor;
varying vec3 vWorld;
varying float vDepth; // 0 at the top of the road, 1 at the bottom
varying float vHot;   // 1 on the layer being printed

void main() {
  float feature = mod(aFeature, 16.0);
  float tool = floor(aFeature / 16.0 + 0.01);
#ifdef GHOST
  // Only the outer shell of unprinted layers: a clean silhouette instead of a haze.
  bool shell = abs(feature - 1.0) < 0.5 || abs(feature - 3.0) < 0.5;
  if (aInfo.x <= uLayer || !shell) { gl_Position = vec4(0.0, 0.0, 2.0, 1.0); return; } // clipped away
#endif
  vec3 d = aEnd - aStart;
  float len = length(d.xy);
  vec2 dir = len > 1e-6 ? d.xy / len : vec2(1.0, 0.0);
  vec2 side = vec2(-dir.y, dir.x);

  vec3 p = mix(aStart, aEnd, position.x);
  p.xy += dir * (uWidth * 0.5 * (position.x * 2.0 - 1.0)); // overlap neighbours at joints
  p.xy += side * (position.y * uWidth);
  p.z += position.z * aInfo.y;

  vec3 n = normal;
  // Fake a rounded bead: bend side normals up/down with height, crown the top.
  if (n.z < 0.5) n.z = (position.z + 0.5) * 1.4;
  else n.y = position.y * 1.1;
  vNormal = vec3(dir * n.x + side * n.y, n.z);

  vColor = uColorMode > 0.5
    ? uPalette[int(clamp(feature, 0.0, ${PALETTE_SIZE - 1}.0))]
    : uFilament[int(clamp(tool, 0.0, ${PALETTE_SIZE - 1}.0))];
  vHot = 0.0;
#ifndef GHOST
  if (uHighlight > 0.5 && abs(aInfo.x - uLayer) < 0.5) vHot = 1.0;
#endif
  vDepth = -position.z;
  vWorld = p;

  gl_Position = projectionMatrix * viewMatrix * vec4(p, 1.0);
}
`;

const FRAG = /* glsl */ `
uniform float uAlpha;
uniform vec3 uGhostColor;
uniform vec3 uHighlightColor;
varying vec3 vNormal;
varying vec3 vColor;
varying vec3 vWorld;
varying float vDepth;
varying float vHot;

void main() {
#ifdef GHOST
  gl_FragColor = vec4(uGhostColor, uAlpha);
#else
  vec3 n = normalize(vNormal);
  vec3 v = normalize(cameraPosition - vWorld);
  vec3 key = normalize(vec3(-0.55, -0.65, 0.75)); // above front-left
  vec3 fill = normalize(vec3(0.7, 0.5, 0.3));     // back-right
  vec3 base = max(vColor, vec3(0.035));           // keep black filament readable

  float hemi = 0.5 + 0.5 * n.z;
  float diff = max(dot(n, key), 0.0);
  float spec = pow(max(dot(n, normalize(key + v)), 0.0), 48.0);
  float rim = pow(1.0 - max(dot(n, v), 0.0), 3.0);
  float ao = mix(1.0, 0.55, vDepth * vDepth);     // groove between layer lines

  vec3 c = base * (0.22 + 0.18 * hemi + 0.75 * diff + 0.18 * max(dot(n, fill), 0.0)) * ao;
  c += vec3(0.30) * spec * ao + base * 0.25 * rim;

  // Layer being printed: hot glowing filament.
  c = mix(c, uHighlightColor * (0.75 + 0.5 * diff) + spec * 0.4, vHot * 0.75);
  gl_FragColor = vec4(c, 1.0);
#endif
  #include <colorspace_fragment>
}
`;

interface ThemeColors {
  bed: number;
  minor: number;
  major: number;
  border: number;
  ghost: number;
  shadow: number;
}
const THEMES: Record<"dark" | "light", ThemeColors> = {
  dark: { bed: 0x1f2124, minor: 0x27292d, major: 0x33363b, border: 0x5a5f67, ghost: 0xc9d4e3, shadow: 0.55 },
  light: { bed: 0xd6d8dc, minor: 0xcbcdd2, major: 0xb6b9bf, border: 0x7e838b, ghost: 0x4a5566, shadow: 0.3 },
};

export type ColorMode = "filament" | "feature";

export class Viewer {
  private container: HTMLElement;
  private renderer: THREE.WebGLRenderer;
  private scene = new THREE.Scene();
  private camera = new THREE.PerspectiveCamera(35, 1, 0.5, 5000);
  private controls: OrbitControls;
  private ro: ResizeObserver;
  private frame = 0;

  private bedGroup = new THREE.Group();
  private bedSize = { w: 180, h: 180 };
  private dark = true;

  private model: Model | null = null;
  private solid: THREE.Mesh | null = null;
  private ghost: THREE.Mesh | null = null;
  private solidGeo: THREE.InstancedBufferGeometry | null = null;
  private ghostGeo: THREE.InstancedBufferGeometry | null = null;
  private box = roadGeometry();
  private bounds = new THREE.Box3();

  /** Last value from setLayer; kept across setModel. Default shows the whole model. */
  private current = Number.MAX_SAFE_INTEGER;
  private done = true;
  private ghostOn = true;

  private uniforms = {
    uWidth: { value: ROAD_WIDTH },
    uLayer: { value: 0 },
    uHighlight: { value: 0 },
    uAlpha: { value: GHOST_ALPHA },
    uColorMode: { value: 0 },
    uPalette: { value: [] as THREE.Color[] },
    uFilament: { value: Array.from({ length: PALETTE_SIZE }, () => new THREE.Color(DEFAULT_FILAMENT)) },
    uHighlightColor: { value: new THREE.Color("#ff7a1a") },
    uGhostColor: { value: new THREE.Color(0xffffff) },
  };
  private solidMat: THREE.ShaderMaterial;
  private ghostMat: THREE.ShaderMaterial;

  constructor(container: HTMLElement) {
    this.container = container;
    for (let i = 0; i < PALETTE_SIZE; i++) {
      const f = FEATURES.find((x) => x.id === i);
      this.uniforms.uPalette.value.push(new THREE.Color(f ? f.color : "#8c8a85")); // sRGB hex → linear
    }

    this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    this.renderer.setClearColor(0x000000, 0);
    this.renderer.outputColorSpace = THREE.SRGBColorSpace;
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    this.renderer.sortObjects = false; // draw order = scene order: bed, solid, ghost
    const canvas = this.renderer.domElement;
    canvas.style.display = "block";
    canvas.style.width = "100%";
    canvas.style.height = "100%";
    canvas.style.outline = "none";
    container.appendChild(canvas);

    this.solidMat = new THREE.ShaderMaterial({
      vertexShader: VERT,
      fragmentShader: FRAG,
      uniforms: this.uniforms,
    });
    this.ghostMat = new THREE.ShaderMaterial({
      vertexShader: VERT,
      fragmentShader: FRAG,
      uniforms: this.uniforms,
      defines: { GHOST: "" },
      transparent: true,
      depthWrite: false,
    });

    this.camera.up.set(0, 0, 1);
    this.controls = new OrbitControls(this.camera, canvas);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.15;
    this.controls.screenSpacePanning = true;
    this.controls.zoomToCursor = true;
    this.controls.addEventListener("change", this.requestRender);

    this.scene.add(this.bedGroup);
    this.dark = window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? true;
    this.buildBed();

    this.ro = new ResizeObserver(() => this.resize());
    this.ro.observe(container);
    this.resize();
    this.resetCamera();
  }

  setModel(model: Model | null): void {
    this.clearModel();
    this.model = model;
    if (model) {
      this.bedSize = { w: model.bedW > 0 ? model.bedW : 180, h: model.bedH > 0 ? model.bedH : 180 };
      this.uniforms.uFilament.value.forEach((c, i) => c.set(model.colors[i] ?? model.colors[0] ?? DEFAULT_FILAMENT));
      this.buildModel(model);
    }
    this.buildBed();
    this.applyLayer();
    this.resetCamera();
  }

  /** Layers < current are solid; `current` is highlighted unless done; layers > current are ghosted. */
  setLayer(current: number, done: boolean): void {
    this.current = Number.isFinite(current) ? Math.floor(current) : Number.MAX_SAFE_INTEGER;
    this.done = done;
    this.applyLayer();
    this.requestRender();
  }

  /** Real filament colors from the G-code, or one color per line type. */
  setColorMode(mode: ColorMode): void {
    this.uniforms.uColorMode.value = mode === "feature" ? 1 : 0;
    this.requestRender();
  }

  setGhost(on: boolean): void {
    this.ghostOn = on;
    this.applyLayer();
    this.requestRender();
  }

  /** Optional: match bed/grid colors to the UI theme (defaults to the OS preference). */
  setTheme(dark: boolean): void {
    if (dark === this.dark) return;
    this.dark = dark;
    this.buildBed();
    this.requestRender();
  }

  resetCamera(): void {
    const box = new THREE.Box3();
    if (this.model && !this.bounds.isEmpty()) box.copy(this.bounds);
    else box.set(new THREE.Vector3(0, 0, 0), new THREE.Vector3(this.bedSize.w, this.bedSize.h, 0));
    const center = box.getCenter(new THREE.Vector3());
    const radius = Math.max(box.getSize(new THREE.Vector3()).length() / 2, 5);

    const vfov = THREE.MathUtils.degToRad(this.camera.fov);
    const hfov = 2 * Math.atan(Math.tan(vfov / 2) * this.camera.aspect);
    const dist = (radius / Math.sin(Math.min(vfov, hfov) / 2)) * 1.05;
    const dir = new THREE.Vector3(-0.55, -1, 0.8).normalize(); // front-left, above
    this.controls.target.copy(center);
    this.camera.position.copy(center).addScaledVector(dir, dist);
    this.controls.maxDistance = Math.max(this.bedSize.w, this.bedSize.h) * 6;
    this.controls.update();
    this.requestRender();
  }

  dispose(): void {
    cancelAnimationFrame(this.frame);
    this.frame = 0;
    this.ro.disconnect();
    this.controls.removeEventListener("change", this.requestRender);
    this.controls.dispose();
    this.clearModel(true);
    this.clearBed();
    this.box.dispose();
    this.solidMat.dispose();
    this.ghostMat.dispose();
    this.renderer.dispose();
    this.renderer.forceContextLoss();
    this.renderer.domElement.remove();
  }

  // ---------------------------------------------------------------- internals

  private requestRender = () => {
    if (!this.frame) this.frame = requestAnimationFrame(this.render);
  };

  private render = () => {
    this.frame = 0;
    const moving = this.controls.update(); // damping; may re-request via "change"
    const dist = this.camera.position.distanceTo(this.controls.target);
    const span = Math.max(this.bedSize.w, this.bedSize.h);
    this.camera.near = THREE.MathUtils.clamp(dist * 0.005, 0.05, 10);
    this.camera.far = dist + span * 4;
    this.camera.updateProjectionMatrix();
    this.renderer.render(this.scene, this.camera);
    if (moving) this.requestRender();
  };

  private resize() {
    const w = Math.max(1, this.container.clientWidth);
    const h = Math.max(1, this.container.clientHeight);
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    this.renderer.setSize(w, h, false);
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
    this.requestRender();
  }

  private applyLayer() {
    const m = this.model;
    if (!m || !this.solidGeo || !this.ghostGeo || !this.solid || !this.ghost) return;
    const last = m.layerCount - 1;
    const cur = Math.min(this.current, last);
    this.uniforms.uLayer.value = cur;
    this.uniforms.uHighlight.value = this.done ? 0 : 1;
    this.solidGeo.instanceCount = cur < 0 ? 0 : m.offsets[cur + 1];
    this.ghost.visible = this.ghostOn && cur < last;
  }

  private buildModel(m: Model) {
    const n = m.segCount;
    const inter = new THREE.InstancedInterleavedBuffer(m.pos.subarray(0, n * 6), 6, 1);
    const aStart = new THREE.InterleavedBufferAttribute(inter, 3, 0);
    const aEnd = new THREE.InterleavedBufferAttribute(inter, 3, 3);
    const aFeature = new THREE.InstancedBufferAttribute(m.feature.subarray(0, n), 1, false);

    // Per-segment layer index and road height.
    const sorted = Array.from(m.height.filter((h) => h > 0.01 && h < 2)).sort((a, b) => a - b);
    const median = sorted.length ? sorted[sorted.length >> 1] : 0.2;
    const info = new Float32Array(n * 2);
    for (let l = 0; l < m.layerCount; l++) {
      let h = m.height[l];
      if (!(h > 0.02) || h > median * 3) h = median;
      const end = Math.min(m.offsets[l + 1], n);
      for (let i = m.offsets[l]; i < end; i++) {
        info[i * 2] = l;
        info[i * 2 + 1] = h;
      }
    }
    const aInfo = new THREE.InstancedBufferAttribute(info, 2);

    const make = () => {
      const g = new THREE.InstancedBufferGeometry();
      g.index = this.box.index;
      g.setAttribute("position", this.box.getAttribute("position"));
      g.setAttribute("normal", this.box.getAttribute("normal"));
      g.setAttribute("aStart", aStart);
      g.setAttribute("aEnd", aEnd);
      g.setAttribute("aFeature", aFeature);
      g.setAttribute("aInfo", aInfo);
      g.instanceCount = n;
      // Never used for culling (frustumCulled = false); set so three never scans it.
      g.boundingSphere = new THREE.Sphere(new THREE.Vector3(), Infinity);
      return g;
    };
    this.solidGeo = make();
    this.ghostGeo = make();
    this.solid = new THREE.Mesh(this.solidGeo, this.solidMat);
    this.ghost = new THREE.Mesh(this.ghostGeo, this.ghostMat);
    for (const mesh of [this.solid, this.ghost]) {
      mesh.frustumCulled = false;
      mesh.matrixAutoUpdate = false;
      this.scene.add(mesh);
    }

    // Bounds for framing; skip layer 0 (start G-code / purge line) when there is more.
    this.bounds.makeEmpty();
    const from = m.layerCount > 1 && m.offsets[1] < n ? m.offsets[1] : 0;
    let x0 = Infinity, y0 = Infinity, z0 = Infinity, x1 = -Infinity, y1 = -Infinity, z1 = -Infinity;
    const p = m.pos;
    for (let i = from * 6, e = n * 6; i < e; i += 3) {
      const x = p[i], y = p[i + 1], z = p[i + 2];
      if (x < x0) x0 = x;
      if (x > x1) x1 = x;
      if (y < y0) y0 = y;
      if (y > y1) y1 = y;
      if (z < z0) z0 = z;
      if (z > z1) z1 = z;
    }
    if (x0 <= x1) this.bounds.set(new THREE.Vector3(x0, y0, Math.min(z0, 0)), new THREE.Vector3(x1, y1, z1));
  }

  /** `all` also frees the shared unit-box buffers (otherwise kept for the next model). */
  private clearModel(all = false) {
    for (const mesh of [this.solid, this.ghost]) if (mesh) this.scene.remove(mesh);
    // Both geometries share their buffers; disposing either frees them on the GPU.
    for (const g of [this.solidGeo, this.ghostGeo]) {
      if (!g) continue;
      if (!all) {
        g.deleteAttribute("position");
        g.deleteAttribute("normal");
        g.index = null;
      }
      g.dispose();
    }
    this.solid = this.ghost = null;
    this.solidGeo = this.ghostGeo = null;
    this.bounds.makeEmpty();
  }

  private clearBed() {
    for (const child of [...this.bedGroup.children]) {
      this.bedGroup.remove(child);
      const o = child as THREE.Mesh | THREE.LineSegments;
      o.geometry.dispose();
      const mat = o.material as THREE.MeshBasicMaterial;
      mat.alphaMap?.dispose();
      mat.dispose();
    }
  }

  private buildBed() {
    this.clearBed();
    const t = this.dark ? THEMES.dark : THEMES.light;
    const { w, h } = this.bedSize;
    this.uniforms.uGhostColor.value.set(t.ghost);

    const plate = new THREE.Mesh(
      new THREE.PlaneGeometry(w, h),
      new THREE.MeshBasicMaterial({
        color: t.bed,
        polygonOffset: true,
        polygonOffsetFactor: 1,
        polygonOffsetUnits: 1,
      }),
    );
    plate.position.set(w / 2, h / 2, -0.02);
    this.bedGroup.add(plate);

    const minor: number[] = [];
    const major: number[] = [];
    const z = -0.01;
    for (let x = 10; x < w - 1e-3; x += 10) (x % 50 === 0 ? major : minor).push(x, 0, z, x, h, z);
    for (let y = 10; y < h - 1e-3; y += 10) (y % 50 === 0 ? major : minor).push(0, y, z, w, y, z);
    const border = [0, 0, z, w, 0, z, w, 0, z, w, h, z, w, h, z, 0, h, z, 0, h, z, 0, 0, z];
    const lines = (pts: number[], color: number) => {
      const g = new THREE.BufferGeometry();
      g.setAttribute("position", new THREE.Float32BufferAttribute(pts, 3));
      this.bedGroup.add(new THREE.LineSegments(g, new THREE.LineBasicMaterial({ color })));
    };
    lines(minor, t.minor);
    lines(major, t.major);
    lines(border, t.border);

    // Soft contact shadow under the model.
    if (this.model && !this.bounds.isEmpty()) {
      const size = this.bounds.getSize(new THREE.Vector3());
      const center = this.bounds.getCenter(new THREE.Vector3());
      const shadow = new THREE.Mesh(
        new THREE.PlaneGeometry(size.x * 1.5 + 8, size.y * 1.5 + 8),
        new THREE.MeshBasicMaterial({
          alphaMap: shadowTexture(),
          color: 0x000000,
          transparent: true,
          opacity: t.shadow,
          depthWrite: false,
        }),
      );
      shadow.position.set(center.x, center.y, -0.005);
      this.bedGroup.add(shadow);
    }
  }
}

/** Radial falloff used as the alpha map of the contact shadow. */
function shadowTexture(): THREE.Texture {
  const size = 128;
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = size;
  const ctx = canvas.getContext("2d")!;
  const g = ctx.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, size / 2);
  g.addColorStop(0, "#fff");
  g.addColorStop(0.55, "#8c8c8c");
  g.addColorStop(1, "#000");
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, size, size);
  return new THREE.CanvasTexture(canvas);
}

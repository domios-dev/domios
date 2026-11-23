import {
  BufferAttribute,
  BufferGeometry,
  CustomBlending,
  Group,
  LineSegments,
  Mesh,
  OneFactor,
  OneMinusSrcAlphaFactor,
  PerspectiveCamera,
  PlaneGeometry,
  Points,
  Scene,
  ShaderMaterial,
  Vector2,
  Vector3,
  WebGLRenderer,
} from "three";
import { readMemoryGraphPalette } from "./holoPalette";
import { EDGES, egoAlpha, hash, HUB, NODES, rankFrom } from "../lab/labGraph";
import { sampleHoloTopology, type HoloBudget } from "./holoBudget";
import { centreOf, DEPTH, FOV, isTurned, TAN, volumeOf } from "./holoVolume";
import { HOLO_DARK, HOLO_LIGHT, holoRgb, LIGHT_BOOST, type HoloCast, type HoloState, type Layout, type Rgb } from "./holoTheme";

// The memory graph as a volumetric hologram, on three.js. Every node and edge carries both its flat layout position (the
// 2D graph the neighbouring cuts draw) and its place in the volume; the vertex shader moves between them from a few
// uniforms, so a frame costs one upload of uniforms and four draw calls whatever t is. Loaded as its own chunk.

/** Seconds one node takes to fly from the plane to its place in the volume. */
const FLIGHT = 0.62;
/** The last node leaves this long after the first. */
const SPREAD = 0.34;

const N = NODES.length;

const RANK = rankFrom(HUB);
const LOG_N = Math.log(N);
/** The burst leaves the first memory and runs outward by hop distance. */
const DELAY = Float32Array.from(RANK, (r) => SPREAD * (Math.log(1 + r) / LOG_N) ** 1.25);
const EDGE_BUCKET = EDGES.map((_, i) => (hash(i) % 4) / 3);

// --- shaders -----------------------------------------------------------------------------------------------------------

const MOTION = /* glsl */ `
uniform float uBurst;
uniform float uFlatten;
uniform float uHolo;
uniform float uScan;
uniform float uDist;
// How far flashes brighten colour: fully on a dark page, barely on a light one, where brightening reads as fading.
uniform float uBoost;
attribute vec3 aFlat;
attribute vec3 aVol;
attribute float aDelay;
uniform float uFlight;
uniform float uOvershoot;
uniform float uStagger;
uniform float uCollapse;
uniform vec3 uFrom;
uniform float uWave;
uniform float uArrival;
uniform vec3 uFreshFrom;
attribute float aFresh;
float flash;
float scanGlow;
float waveGlow;
float launched;
vec3 holoPos() {
  float delay = aDelay * uStagger;
  float k = clamp((uBurst - delay) / uFlight, 0.0, 1.0);
  float m = k - 1.0;
  // ease-out-back: overshoots its place (by about a fifth at uOvershoot 1.9) and settles, the "bam"
  float b = 1.0 + (uOvershoot + 1.0) * m * m * m + uOvershoot * m * m;
  vec3 start = mix(aFlat, uFrom, uCollapse);
  vec3 p = mix(start, aVol, b);
  p = mix(p, aFlat, uFlatten);
  // The new memory streaks in from far outside the frame and lands in its place.
  float arrive = 1.0 - pow(1.0 - clamp(uArrival, 0.0, 1.0), 3.0);
  p += aFresh * (1.0 - arrive) * uFreshFrom;
  float since = uBurst - delay;
  flash = since > 0.0 ? exp(-since * 3.2) : 0.0;
  flash += aFresh * exp(-pow((uArrival - 1.0) / 0.12, 2.0)) * 0.7;
  launched = mix(1.0, smoothstep(0.0, 0.05, since), uCollapse);
  scanGlow = exp(-pow((p.y - uScan) / 34.0, 2.0));
  // A narrow band of light, softened: it should read as a pulse running through, never as a flash of the whole volume.
  waveGlow = 0.8 * exp(-pow((uWave - aDelay * 5.0) / 0.2, 2.0));
  return p;
}
// Nearer is brighter, farther fades into the volume: the depth cue a still frame needs. 1 on the flat plane.
float depthCue(vec4 mv) {
  float near = (uDist + mv.z) / ${DEPTH.toFixed(1)};
  return mix(1.0, clamp(1.0 + 0.8 * near, 0.3, 1.45), uHolo * (1.0 - uFlatten));
}
`;

const POINT_VERTEX = /* glsl */ `
${MOTION}
uniform float uProj;
uniform float uMinPx;
uniform float uMaxPx;
uniform float uOpacity;
uniform float uEgo;
uniform vec3 uEgoColor;
attribute vec3 aColA;
attribute vec3 aColB;
attribute float aSize;
attribute float aAlphaA;
attribute float aAlphaB;
attribute float aHub;
varying vec3 vColor;
varying float vAlpha;
void main() {
  vec3 p = holoPos();
  vec4 mv = modelViewMatrix * vec4(p, 1.0);
  // Every lit link has to land on something you can see: the memory's neighbours light up with it, even the faint
  // entities, larger, fully opaque and warmed towards its orange.
  float lit = min(1.0, aHub) * uEgo;
  float lift = 1.0 + 0.9 * aHub * uEgo + 2.2 * lit;
  // A lit neighbour never drops below a clearly visible dot, however faint the entity it is.
  gl_PointSize = max(clamp(aSize * lift * uProj / -mv.z, uMinPx, uMaxPx), lit * uMinPx * 6.0) * 2.6;
  vColor = mix(aColA, aColB, uHolo) * (1.0 + uBoost * uHolo * (1.5 * flash + 0.9 * scanGlow + 1.3 * waveGlow));
  vColor = mix(vColor, uEgoColor * 1.15, 0.65 * lit);
  vAlpha = max(mix(aAlphaA, aAlphaB, uHolo) * depthCue(mv), lit) * uOpacity * launched;
  gl_Position = projectionMatrix * mv;
}
`;

const POINT_FRAGMENT = /* glsl */ `
uniform float uGlow;
varying vec3 vColor;
varying float vAlpha;
void main() {
  vec2 d = gl_PointCoord * 2.0 - 1.0;
  float r = length(d);
  if (r > 1.0) discard;
  float core = 1.0 - smoothstep(0.30, 0.39, r);
  float halo = exp(-r * r * 7.0) * 0.6 * uGlow;
  float a = clamp(core + halo, 0.0, 1.0) * vAlpha;
  gl_FragColor = vec4(vColor * a, a);
}
`;

const LINE_VERTEX = /* glsl */ `
${MOTION}
uniform float uOpacity;
attribute vec3 aColA;
attribute vec3 aColB;
attribute float aAlphaA;
attribute float aAlphaB;
varying vec3 vColor;
varying float vAlpha;
void main() {
  vec3 p = holoPos();
  vColor = mix(aColA, aColB, uHolo) * (1.0 + uBoost * uHolo * (0.8 * flash + 1.2 * scanGlow + 2.6 * waveGlow));
  vec4 mv = modelViewMatrix * vec4(p, 1.0);
  vAlpha = mix(aAlphaA, aAlphaB, uHolo) * uOpacity * depthCue(mv) * launched * (1.0 + 2.5 * waveGlow * uHolo);
  gl_Position = projectionMatrix * mv;
}
`;

const LINE_FRAGMENT = /* glsl */ `
varying vec3 vColor;
varying float vAlpha;
void main() {
  gl_FragColor = vec4(vColor * vAlpha, vAlpha);
}
`;

const RING_VERTEX = /* glsl */ `
varying vec2 vLocal;
void main() {
  vLocal = position.xy;
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}
`;

const RING_FRAGMENT = /* glsl */ `
uniform float uRadius;
uniform float uWidth;
uniform float uAlpha;
uniform vec3 uColor;
varying vec2 vLocal;
void main() {
  float d = length(vLocal);
  float band = exp(-pow((d - uRadius) / uWidth, 2.0));
  float inner = d < uRadius ? exp(-pow((d - uRadius) / (uWidth * 6.0), 2.0)) * 0.25 : 0.0;
  float a = (band + inner) * uAlpha;
  gl_FragColor = vec4(uColor * a, a);
}
`;

// --- the scene ---------------------------------------------------------------------------------------------------------

const RING_REACH = 1500;

export class Holo {
  private scene = new Scene();
  private camera = new PerspectiveCamera(FOV, 16 / 9, 1, 30000);
  private pivot = new Group();
  private body = new Group();
  private materials: ShaderMaterial[] = [];
  private geometries: BufferGeometry[] = [];
  private shared = {
    uBurst: { value: -1 }, uFlatten: { value: 0 }, uHolo: { value: 0 }, uScan: { value: -1e6 }, uDist: { value: 1000 }, uBoost: { value: 1 },
    uFlight: { value: FLIGHT }, uOvershoot: { value: 1.9 }, uStagger: { value: 1 }, uCollapse: { value: 0 }, uFrom: { value: new Vector3() },
    uWave: { value: -100 }, uArrival: { value: 1 }, uFreshFrom: { value: new Vector3(-2200, 1300, 1600) },
  };
  private points: ShaderMaterial;
  private lines: ShaderMaterial;
  private ego: ShaderMaterial;
  private ring: ShaderMaterial;
  private ringMesh: Mesh;
  private centre: [number, number];

  private constructor(private renderer: WebGLRenderer, private layout: Layout, readonly light: boolean, cast: HoloCast, budget?: HoloBudget) {
    const palette = readMemoryGraphPalette();
    const sample = budget ? sampleHoloTopology(N, EDGES, cast.ego, budget) : null;
    const edges = sample ? sample.edges.map((index) => EDGES[index]) : EDGES;
    const egoEdges = edges.filter(([a, b]) => a === cast.ego || b === cast.ego);
    const egoSet = new Set(egoEdges.flatMap(([a, b]) => [a, b]));
    const fresh = (i: number) => (i === cast.fresh ? 1 : 0);
    const holo = light ? HOLO_LIGHT : HOLO_DARK;
    this.shared.uBoost.value = light ? 0.2 : 1;
    const blending = { blending: CustomBlending, blendSrc: OneFactor, blendDst: light ? OneMinusSrcAlphaFactor : OneFactor, blendSrcAlpha: OneFactor, blendDstAlpha: OneMinusSrcAlphaFactor };
    const material = (vertexShader: string, fragmentShader: string, uniforms: Record<string, { value: unknown }>) => {
      const m = new ShaderMaterial({ vertexShader, fragmentShader, uniforms: { ...this.shared, ...uniforms }, transparent: true, depthTest: false, depthWrite: false, ...blending });
      this.materials.push(m);
      return m;
    };

    const flat = (i: number) => layout[i];
    this.centre = centreOf(layout);

    // Nodes: entities first so memories draw over them.
    const order = (sample?.points ?? [...NODES.keys()]).sort((a, b) => Number(NODES[a][2] >= 0) - Number(NODES[b][2] >= 0));
    const nodeGeo = this.geometry(order.length);
    const put = (name: string, size: number, fill: (i: number, out: Float32Array, at: number) => void) => {
      const data = new Float32Array(order.length * size);
      order.forEach((node, slot) => fill(node, data, slot * size));
      nodeGeo.setAttribute(name, new BufferAttribute(data, size));
    };
    const turned = isTurned(layout);
    const volOf = (i: number) => volumeOf(i, layout, turned);
    const vol = (i: number, out: Float32Array, at: number) => {
      out.set(volOf(i), at);
    };
    put("position", 3, vol);
    put("aVol", 3, vol);
    put("aFlat", 3, (i, out, at) => ((out[at] = flat(i)[0]), (out[at + 1] = -flat(i)[1]), (out[at + 2] = 0)));
    put("aDelay", 1, (i, out, at) => (out[at] = DELAY[i]));
    // The flat graph wears the same palette, so colour stays put; only brightness and glow change with the burst.
    const tint = (i: number, out: Float32Array, at: number) => out.set(holoRgb(NODES[i][2] >= 0 ? NODES[i][2] : -1, light), at);
    put("aColA", 3, tint);
    put("aColB", 3, tint);
    put("aSize", 1, (i, out, at) => {
      const [, , kind, degree] = NODES[i];
      out[at] = kind >= 0 ? 3.4 + Math.sqrt(degree) * 0.6 : 1.0 + Math.sqrt(degree) * 0.2;
    });
    put("aAlphaA", 1, (i, out, at) => (out[at] = NODES[i][2] >= 0 ? 1 : 0.55));
    put("aAlphaB", 1, (i, out, at) => (out[at] = NODES[i][2] >= 0 ? 1 : light ? Math.min(0.95, 0.5 * LIGHT_BOOST) : 0.42));
    put("aHub", 1, (i, out, at) => (out[at] = i === cast.ego ? 1.6 : egoSet.has(i) ? 1 : 0));
    put("aFresh", 1, (i, out, at) => (out[at] = fresh(i)));
    this.points = material(POINT_VERTEX, POINT_FRAGMENT, { uProj: { value: 1 }, uMinPx: { value: 1 }, uMaxPx: { value: 40 }, uOpacity: { value: 1 }, uEgo: { value: 0 }, uEgoColor: { value: holo.ego }, uGlow: { value: light ? 0 : 1 } });
    // Nodes lay over the additive body of relations rather than add to it, so an orange memory stays orange.
    this.points.blendDst = OneMinusSrcAlphaFactor;
    const pointMesh = new Points(nodeGeo, this.points);

    // Relations: one buffer, two vertices each.
    const lineGeo = (pairs: ReadonlyArray<readonly [number, number]>, colA: (e: number) => Rgb, colB: Rgb, alphaA: (e: number) => number, alphaB: (e: number) => number) => {
      const geo = this.geometry(pairs.length * 2);
      const n = pairs.length * 2;
      const attrs = { position: new Float32Array(n * 3), aVol: new Float32Array(n * 3), aFlat: new Float32Array(n * 3), aDelay: new Float32Array(n), aFresh: new Float32Array(n), aColA: new Float32Array(n * 3), aColB: new Float32Array(n * 3), aAlphaA: new Float32Array(n), aAlphaB: new Float32Array(n) };
      pairs.forEach((pair, e) => {
        pair.forEach((i, end) => {
          const v = e * 2 + end;
          const [x, y] = flat(i);
          attrs.aFlat.set([x, -y, 0], v * 3);
          attrs.aVol.set(volOf(i), v * 3);
          attrs.position.set(volOf(i), v * 3);
          attrs.aDelay[v] = DELAY[i];
          attrs.aFresh[v] = fresh(i);
          attrs.aColA.set(colA(e), v * 3);
          attrs.aColB.set(colB, v * 3);
          attrs.aAlphaA[v] = alphaA(e);
          attrs.aAlphaB[v] = alphaB(e);
        });
      });
      for (const [name, data] of Object.entries(attrs)) geo.setAttribute(name, new BufferAttribute(data, data.length === n ? 1 : 3));
      return geo;
    };
    // As labGraph draws the flat graph, so the burst starts from exactly that look.
    const bucket = (index: number) => EDGE_BUCKET[sample?.edges[index] ?? index];
    const flatAlpha = (e: number) => (light ? Math.min(0.24 * LIGHT_BOOST, palette.realEdgeAlpha(bucket(e)) * 2.2 * 0.55 * LIGHT_BOOST) : palette.realEdgeAlpha(bucket(e)) * 2.2 * 1.5);
    const holoAlpha = (e: number) => (light ? (0.05 + 0.07 * bucket(e)) * LIGHT_BOOST : 0.035 + 0.06 * bucket(e));
    this.lines = material(LINE_VERTEX, LINE_FRAGMENT, { uOpacity: { value: 1 } });
    const lineMesh = new LineSegments(lineGeo(edges, () => holo.edge, holo.edge, flatAlpha, holoAlpha), this.lines);
    this.ego = material(LINE_VERTEX, LINE_FRAGMENT, { uOpacity: { value: 0 } });
    const egoMesh = new LineSegments(lineGeo(egoEdges, () => holo.ego, holo.ego, () => egoAlpha(palette), () => (light ? 0.95 : 0.85)), this.ego);

    // The shockwave, lying in the graph's own plane around the first memory.
    this.ring = new ShaderMaterial({
      vertexShader: RING_VERTEX,
      fragmentShader: RING_FRAGMENT,
      uniforms: { uRadius: { value: 0 }, uWidth: { value: 10 }, uAlpha: { value: 0 }, uColor: { value: [1, 0.7, 0.3] } },
      transparent: true,
      depthTest: false,
      depthWrite: false,
      ...blending,
    });
    this.materials.push(this.ring);
    const ringGeo = new PlaneGeometry(RING_REACH * 2.4, RING_REACH * 2.4);
    this.geometries.push(ringGeo);
    this.ringMesh = new Mesh(ringGeo, this.ring);
    this.ringMesh.renderOrder = -1;

    for (const object of [lineMesh, egoMesh, pointMesh, this.ringMesh]) object.frustumCulled = false;
    lineMesh.renderOrder = 0;
    egoMesh.renderOrder = 1;
    pointMesh.renderOrder = 2;
    this.body.add(this.ringMesh, lineMesh, egoMesh, pointMesh);
    // The volume turns about its own centre.
    this.pivot.position.set(this.centre[0], -this.centre[1], 0);
    this.body.position.set(-this.centre[0], this.centre[1], 0);
    this.pivot.add(this.body);
    this.scene.add(this.pivot);
  }

  private geometry(_count: number) {
    const geo = new BufferGeometry();
    this.geometries.push(geo);
    return geo;
  }

  /** `spill`: the big canvas past the frame, seen faded and masked, so it skips multisampling and is kept light. */
  static create(canvas: HTMLCanvasElement, layout: Layout, light: boolean, cast: HoloCast, spill = false, budget?: HoloBudget): Holo | null {
    try {
      // Dense screens hide aliasing on their own; multisampling there only costs GPU memory a phone doesn't have.
      const antialias = !budget && !spill && window.devicePixelRatio < 2;
      const renderer = new WebGLRenderer({ canvas, antialias, alpha: true, premultipliedAlpha: true, depth: false, stencil: false, powerPreference: spill ? "default" : "high-performance" });
      renderer.setClearColor(0x000000, 0);
      return new Holo(renderer, layout, light, cast, budget);
    } catch {
      return null;
    }
  }

  /**
   * Draws the frame `width` x `height` (stage pixels). With `spill`, the canvas is that frame plus `reachX`/`reachY` on
   * every side: the same picture continues past the frame at the same scale, and the frame itself is left empty.
   */
  render(state: HoloState, width: number, height: number, density: number, spill?: { reachX: number; reachY: number }) {
    const { renderer, camera } = this;
    const fullW = width + 2 * (spill?.reachX ?? 0);
    const fullH = height + 2 * (spill?.reachY ?? 0);
    if (renderer.getPixelRatio() !== density) renderer.setPixelRatio(density);
    const size = renderer.getSize(this.size);
    if (size.x !== fullW || size.y !== fullH) renderer.setSize(fullW, fullH, false);

    const { view } = state;
    camera.aspect = fullW / fullH;
    // A taller canvas around the same centre keeps the frame's scale by widening the field of view to match.
    camera.fov = (2 * Math.atan(TAN * (fullH / height)) * 180) / Math.PI;
    const distance = height / 2 / (TAN * view.scale);
    this.shared.uDist.value = distance;
    camera.position.set(view.x, -view.y, distance);
    camera.lookAt(view.x, -view.y, 0);
    camera.updateProjectionMatrix();

    this.pivot.rotation.set(state.pitch, state.yaw, 0);
    const s = this.shared;
    s.uBurst.value = state.burst;
    s.uFlatten.value = state.flatten;
    s.uHolo.value = state.holo;
    s.uScan.value = state.scan === null ? -1e6 : -state.scan;
    s.uFlight.value = state.flight ?? FLIGHT;
    s.uOvershoot.value = state.overshoot ?? 1.9;
    s.uStagger.value = state.stagger ?? 1;
    s.uCollapse.value = state.collapse ?? 0;
    const from = state.from ?? [0, 0];
    if (state.fromNode !== undefined) s.uFrom.value.set(...volumeOf(state.fromNode, this.layout));
    else s.uFrom.value.set(from[0], -from[1], 0);
    this.ringMesh.position.set(from[0], -from[1], 0);
    s.uWave.value = state.wave ?? -100;
    s.uArrival.value = state.arrival ?? 1;

    const u = this.points.uniforms;
    u.uProj.value = (height * density) / 2 / TAN;
    u.uMinPx.value = 1.15 * density;
    u.uMaxPx.value = 26 * density;
    u.uOpacity.value = state.opacity;
    u.uEgo.value = state.ego;
    this.lines.uniforms.uOpacity.value = state.opacity;
    this.ego.uniforms.uOpacity.value = state.opacity * state.ego;

    const ring = this.ring.uniforms;
    if (state.ring === null || state.ring >= 1) {
      this.ringMesh.visible = false;
    } else {
      const k = state.ring;
      const holo = this.light ? HOLO_LIGHT : HOLO_DARK;
      this.ringMesh.visible = true;
      ring.uRadius.value = RING_REACH * (1 - (1 - k) ** 2.4);
      ring.uWidth.value = 5 + 16 * k;
      ring.uAlpha.value = state.opacity * (1 - k) ** 1.8 * (this.light ? 0.8 : 0.55);
      ring.uColor.value = holo.ring[0].map((c, i) => c + (holo.ring[1][i] - c) * k);
    }
    renderer.render(this.scene, camera);
    if (spill) {
      // The frame keeps its own copy; the spill shows only what lies past its edges.
      renderer.setScissorTest(true);
      renderer.setScissor(spill.reachX, spill.reachY, width, height);
      renderer.clear();
      renderer.setScissorTest(false);
    }
  }

  private size = new Vector2();

  /**
   * Frees the GPU buffers. `release` also gives the WebGL context back at once: phones hold only a few contexts and little
   * GPU memory, and a context left for the garbage collector after its cut ends can get the page reloaded by the browser.
   * Leave it false when a new renderer is about to take the same canvas (its context can't be had again once lost).
   */
  dispose(release = true) {
    for (const geo of this.geometries) geo.dispose();
    for (const m of this.materials) m.dispose();
    this.renderer.dispose();
    if (release) this.renderer.forceContextLoss();
  }
}


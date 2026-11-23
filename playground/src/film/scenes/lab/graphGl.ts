/** Per-node values for one layer, packed as x, y, radius (device pixels) and visibility, one texel per node. */
export const NODE_TEXELS_PER_ROW = 1024;

export interface GlEdges {
  /** a, b, alpha bucket per edge. */
  pairs: Float32Array;
  count: number;
}

export interface GlLayer {
  nodes: Float32Array;
  /** Per-edge visibility multiplier on top of the endpoints', or none for all ones. */
  edgeAlpha?: Float32Array;
  edgeRgb: [number, number, number];
  entityRgb: [number, number, number];
  bucketAlpha: [number, number, number, number];
  edgeHalfWidth: number;
  extra?: { edges: GlEdges; alpha: number; halfWidth: number; rgb: [number, number, number] };
  entityAlpha: number;
  opacity: number;
}

const OFFSCREEN = "gl_Position = vec4(2.0, 2.0, 2.0, 1.0);";

const NODE_FETCH = `
uniform highp sampler2D uNodes;
vec4 node(float index) {
  int i = int(index + 0.5);
  return texelFetch(uNodes, ivec2(i % ${NODE_TEXELS_PER_ROW}, i / ${NODE_TEXELS_PER_ROW}), 0);
}
vec4 clip(vec2 p, vec2 viewport) {
  vec2 c = p / viewport * 2.0 - 1.0;
  return vec4(c.x, -c.y, 0.0, 1.0);
}`;

const EDGE_VS = `#version 300 es
in vec2 aCorner;
in vec3 aEdge;
in float aEdgeAlpha;
uniform vec2 uViewport;
uniform float uHalf;
uniform vec4 uBucket;
out float vAcross;
out float vAlpha;
${NODE_FETCH}
void main() {
  vec4 a = node(aEdge.x);
  vec4 b = node(aEdge.y);
  vAlpha = min(a.w, b.w) * aEdgeAlpha * uBucket[int(aEdge.z + 0.5)];
  vAcross = 0.0;
  if (vAlpha <= 0.0) { ${OFFSCREEN} return; }
  vec2 d = b.xy - a.xy;
  float len = length(d);
  vec2 dir = len > 0.0 ? d / len : vec2(1.0, 0.0);
  float reach = uHalf + 1.0;
  vAcross = aCorner.y * reach;
  gl_Position = clip(mix(a.xy, b.xy, aCorner.x) + vec2(-dir.y, dir.x) * vAcross, uViewport);
}`;

const EDGE_FS = `#version 300 es
precision highp float;
uniform vec3 uColor;
uniform float uHalf;
in float vAcross;
in float vAlpha;
out vec4 color;
void main() {
  float a = vAlpha * clamp(uHalf + 0.5 - abs(vAcross), 0.0, 1.0);
  color = vec4(uColor * a, a);
}`;

const NODE_VS = `#version 300 es
in vec2 aCorner;
in vec2 aNode;
uniform vec2 uViewport;
uniform vec3 uKinds[16];
uniform vec3 uEntity;
uniform float uEntityAlpha;
uniform float uOpacity;
out vec2 vLocal;
out float vRadius;
out vec4 vColor;
${NODE_FETCH}
void main() {
  vec4 n = node(aNode.x);
  bool entity = aNode.y < 0.0;
  float alpha = n.w * (entity ? uEntityAlpha : 1.0) * uOpacity;
  vLocal = vec2(0.0);
  vRadius = n.z;
  vColor = vec4(entity ? uEntity : uKinds[int(aNode.y + 0.5)], alpha);
  if (alpha <= 0.0 || n.z <= 0.0) { ${OFFSCREEN} return; }
  vLocal = aCorner * (n.z + 1.0);
  gl_Position = clip(n.xy + vLocal, uViewport);
}`;

const NODE_FS = `#version 300 es
precision highp float;
in vec2 vLocal;
in float vRadius;
in vec4 vColor;
out vec4 color;
void main() {
  float a = vColor.a * clamp(vRadius + 0.5 - length(vLocal), 0.0, 1.0);
  color = vec4(vColor.rgb * a, a);
}`;

function compile(gl: WebGL2RenderingContext, vs: string, fs: string): WebGLProgram {
  const program = gl.createProgram()!;
  for (const [type, source] of [[gl.VERTEX_SHADER, vs], [gl.FRAGMENT_SHADER, fs]] as const) {
    const shader = gl.createShader(type)!;
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(shader) ?? "shader");
    gl.attachShader(program, shader);
  }
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program) ?? "link");
  return program;
}

const EDGE_QUAD = new Float32Array([0, -1, 1, -1, 1, 1, 0, -1, 1, 1, 0, 1]);
const NODE_QUAD = new Float32Array([-1, -1, 1, -1, 1, 1, -1, -1, 1, 1, -1, 1]);

interface Resources {
  edge: WebGLProgram;
  node: WebGLProgram;
  edgeUniforms: Record<string, WebGLUniformLocation | null>;
  nodeUniforms: Record<string, WebGLUniformLocation | null>;
  texture: WebGLTexture;
  edgeAlpha: WebGLBuffer;
  edgeVaos: Map<GlEdges, WebGLVertexArrayObject>;
  nodeVao: WebGLVertexArrayObject;
  nodeOrder: WebGLBuffer;
}

/**
 * The memory graph on the GPU: every edge in one instanced draw and every node in two, whatever the reveal state, so a
 * frame costs the same few calls on a weak laptop as on a strong one.
 */
export class GraphGl {
  private gl: WebGL2RenderingContext;
  private res: Resources | null = null;
  private lost = false;

  private constructor(
    private canvas: HTMLCanvasElement,
    gl: WebGL2RenderingContext,
    private edges: GlEdges,
    private nodeOrder: Float32Array,
    private entityCount: number,
    /** Uploaded on every render, so a theme change only needs a new value here. */
    public kindRgb: number[],
  ) {
    this.gl = gl;
    canvas.addEventListener("webglcontextlost", this.onLost);
    canvas.addEventListener("webglcontextrestored", this.onRestored);
  }

  private static support: boolean | undefined;

  /**
   * Tried once on a throwaway canvas: a canvas that has asked for WebGL can no longer fall back to 2D. A software GL
   * (a blocklisted GPU) is refused, since there the 2D path is the faster one.
   */
  static supported(): boolean {
    if (GraphGl.support !== undefined) return GraphGl.support;
    const gl = document.createElement("canvas").getContext("webgl2", { failIfMajorPerformanceCaveat: true });
    try {
      if (gl) for (const [vs, fs] of [[EDGE_VS, EDGE_FS], [NODE_VS, NODE_FS]]) compile(gl, vs, fs);
      const info = gl?.getExtension("WEBGL_debug_renderer_info");
      const renderer = info ? String(gl!.getParameter(info.UNMASKED_RENDERER_WEBGL)) : "";
      GraphGl.support = !!gl && !/swiftshader|llvmpipe|software/i.test(renderer);
    } catch {
      GraphGl.support = false;
    }
    gl?.getExtension("WEBGL_lose_context")?.loseContext();
    return GraphGl.support;
  }

  /** `nodeOrder` holds (index, kind) per node, entities (kind -1) first; null when WebGL2 is unavailable. */
  static create(canvas: HTMLCanvasElement, edges: GlEdges, nodeOrder: Float32Array, entityCount: number, kindRgb: number[]): GraphGl | null {
    const gl = GraphGl.supported() ? canvas.getContext("webgl2", { antialias: false, depth: false, stencil: false, premultipliedAlpha: true }) : null;
    if (!gl) return null;
    const renderer = new GraphGl(canvas, gl, edges, nodeOrder, entityCount, kindRgb);
    try {
      renderer.res = renderer.build();
    } catch {
      renderer.dispose();
      return null;
    }
    return renderer;
  }

  private onLost = (event: Event) => {
    event.preventDefault();
    this.lost = true;
    this.res = null;
  };

  private onRestored = () => {
    this.lost = false;
    this.res = this.build();
  };

  private build(): Resources {
    const { gl } = this;
    const edge = compile(gl, EDGE_VS, EDGE_FS);
    const node = compile(gl, NODE_VS, NODE_FS);
    const uniforms = (program: WebGLProgram, names: string[]) => Object.fromEntries(names.map((name) => [name, gl.getUniformLocation(program, name)]));

    const texture = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_2D, texture);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    const rows = Math.ceil(this.nodeOrder.length / 2 / NODE_TEXELS_PER_ROW);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA32F, NODE_TEXELS_PER_ROW, rows, 0, gl.RGBA, gl.FLOAT, null);

    const edgeAlpha = gl.createBuffer()!;
    gl.bindBuffer(gl.ARRAY_BUFFER, edgeAlpha);
    gl.bufferData(gl.ARRAY_BUFFER, this.edges.count * 4, gl.DYNAMIC_DRAW);

    const nodeVao = gl.createVertexArray()!;
    gl.bindVertexArray(nodeVao);
    this.attribute(node, "aCorner", NODE_QUAD, 2, 0);
    const nodeOrder = this.attribute(node, "aNode", this.nodeOrder, 2, 1);
    gl.bindVertexArray(null);

    const res: Resources = {
      edge,
      node,
      edgeUniforms: uniforms(edge, ["uNodes", "uViewport", "uHalf", "uBucket", "uColor"]),
      nodeUniforms: uniforms(node, ["uNodes", "uViewport", "uKinds", "uEntity", "uEntityAlpha", "uOpacity"]),
      texture,
      edgeAlpha,
      edgeVaos: new Map(),
      nodeVao,
      nodeOrder,
    };
    this.res = res;
    this.edgeVao(this.edges);
    return res;
  }

  private attribute(program: WebGLProgram, name: string, data: Float32Array, size: number, divisor: number): WebGLBuffer {
    const { gl } = this;
    const buffer = gl.createBuffer()!;
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    gl.bufferData(gl.ARRAY_BUFFER, data, gl.STATIC_DRAW);
    const location = gl.getAttribLocation(program, name);
    if (location < 0) return buffer;
    gl.enableVertexAttribArray(location);
    gl.vertexAttribPointer(location, size, gl.FLOAT, false, 0, 0);
    gl.vertexAttribDivisor(location, divisor);
    return buffer;
  }

  private edgeVao(edges: GlEdges): WebGLVertexArrayObject {
    const res = this.res!;
    const cached = res.edgeVaos.get(edges);
    if (cached) return cached;
    const { gl } = this;
    const vao = gl.createVertexArray()!;
    gl.bindVertexArray(vao);
    this.attribute(res.edge, "aCorner", EDGE_QUAD, 2, 0);
    this.attribute(res.edge, "aEdge", edges.pairs, 3, 1);
    if (edges === this.edges) {
      const location = gl.getAttribLocation(res.edge, "aEdgeAlpha");
      gl.bindBuffer(gl.ARRAY_BUFFER, res.edgeAlpha);
      gl.vertexAttribPointer(location, 1, gl.FLOAT, false, 0, 0);
      gl.vertexAttribDivisor(location, 1);
    }
    gl.bindVertexArray(null);
    res.edgeVaos.set(edges, vao);
    return vao;
  }

  /** Clears the canvas and draws `layers` in order; `hole` (device pixels, top-left origin) is left empty. */
  render(layers: GlLayer[], hole?: { x: number; y: number; width: number; height: number }) {
    const { gl, canvas } = this;
    const res = this.res;
    if (!res || this.lost) return;
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.enable(gl.BLEND);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, res.texture);
    const rows = Math.ceil(this.nodeOrder.length / 2 / NODE_TEXELS_PER_ROW);
    const viewport = [canvas.width, canvas.height] as const;

    for (const layer of layers) {
      gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, NODE_TEXELS_PER_ROW, rows, gl.RGBA, gl.FLOAT, layer.nodes);

      gl.useProgram(res.edge);
      const eu = res.edgeUniforms;
      gl.uniform1i(eu.uNodes, 0);
      gl.uniform2f(eu.uViewport, ...viewport);
      gl.uniform3f(eu.uColor, ...layer.edgeRgb);
      // One stroked path per bucket never brightens where lines cross; MAX keeps that look across instances.
      gl.blendEquation(gl.MAX);
      gl.bindVertexArray(this.edgeVao(this.edges));
      const location = gl.getAttribLocation(res.edge, "aEdgeAlpha");
      if (layer.edgeAlpha) {
        gl.bindBuffer(gl.ARRAY_BUFFER, res.edgeAlpha);
        gl.bufferSubData(gl.ARRAY_BUFFER, 0, layer.edgeAlpha);
        gl.enableVertexAttribArray(location);
      } else {
        gl.disableVertexAttribArray(location);
        gl.vertexAttrib1f(location, 1);
      }
      gl.uniform1f(eu.uHalf, layer.edgeHalfWidth);
      gl.uniform4f(eu.uBucket, ...layer.bucketAlpha);
      gl.drawArraysInstanced(gl.TRIANGLES, 0, 6, this.edges.count);

      if (layer.extra && layer.extra.alpha > 0) {
        gl.blendEquation(gl.FUNC_ADD);
        gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
        gl.bindVertexArray(this.edgeVao(layer.extra.edges));
        gl.vertexAttrib1f(location, 1);
        gl.uniform3f(eu.uColor, ...layer.extra.rgb);
        gl.uniform1f(eu.uHalf, layer.extra.halfWidth);
        const a = layer.extra.alpha;
        gl.uniform4f(eu.uBucket, a, a, a, a);
        gl.drawArraysInstanced(gl.TRIANGLES, 0, 6, layer.extra.edges.count);
      }

      gl.useProgram(res.node);
      const nu = res.nodeUniforms;
      gl.uniform1i(nu.uNodes, 0);
      gl.uniform2f(nu.uViewport, ...viewport);
      gl.uniform3fv(nu.uKinds, this.kindRgb);
      gl.uniform3f(nu.uEntity, ...layer.entityRgb);
      gl.uniform1f(nu.uEntityAlpha, layer.entityAlpha);
      gl.uniform1f(nu.uOpacity, layer.opacity);
      gl.bindVertexArray(res.nodeVao);
      // Entities share the edge colour, so MAX matches their single filled path; kind nodes paint over in order.
      gl.blendEquation(gl.MAX);
      gl.drawArraysInstanced(gl.TRIANGLES, 0, 6, this.entityCount);
      gl.blendEquation(gl.FUNC_ADD);
      gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
      const order = gl.getAttribLocation(res.node, "aNode");
      gl.bindBuffer(gl.ARRAY_BUFFER, res.nodeOrder);
      gl.vertexAttribPointer(order, 2, gl.FLOAT, false, 0, this.entityCount * 8);
      gl.drawArraysInstanced(gl.TRIANGLES, 0, 6, this.nodeOrder.length / 2 - this.entityCount);
      gl.vertexAttribPointer(order, 2, gl.FLOAT, false, 0, 0);
    }
    gl.bindVertexArray(null);

    if (hole) {
      gl.enable(gl.SCISSOR_TEST);
      gl.scissor(Math.floor(hole.x), Math.floor(canvas.height - hole.y - hole.height), Math.ceil(hole.width), Math.ceil(hole.height));
      gl.clear(gl.COLOR_BUFFER_BIT);
      gl.disable(gl.SCISSOR_TEST);
    }
  }

  /** Frees the context at once; browsers cap live WebGL contexts per page and scenes come and go. */
  dispose() {
    this.canvas.removeEventListener("webglcontextlost", this.onLost);
    this.canvas.removeEventListener("webglcontextrestored", this.onRestored);
    this.res = null;
    this.gl.getExtension("WEBGL_lose_context")?.loseContext();
  }
}

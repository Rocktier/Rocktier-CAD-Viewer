import type { Scene } from "./scene";
import type { Camera } from "./camera";

const VS = `
attribute vec2 a_pos;
attribute vec4 a_col;
uniform mat3 u_m;
uniform vec3 u_auto;
uniform float u_point;
uniform float u_mono;
varying vec4 v_col;
void main() {
  vec3 p = u_m * vec3(a_pos, 1.0);
  gl_Position = vec4(p.xy, 0.0, 1.0);
  gl_PointSize = u_point;
  vec4 c = a_col.a < 0.5 ? vec4(u_auto, 1.0) : a_col;
  // Print export: ignore every entity colour (AutoCAD's monochrome plot style).
  v_col = u_mono > 0.5 ? vec4(u_auto, 1.0) : c;
}
`;

const FS = `
precision mediump float;
varying vec4 v_col;
void main() {
  gl_FragColor = v_col;
}
`;

const STRIDE = 12; // f32 x, f32 y, u8 r,g,b,a

interface LayoutBuffers {
  lines: WebGLBuffer | null;
  points: WebGLBuffer | null;
  tris: WebGLBuffer | null;
  masks: WebGLBuffer | null;
}

export interface DrawOptions {
  autoColor: [number, number, number];
  background: [number, number, number];
  hidden: Set<number>;
  dpr: number;
  /** Draw everything in `autoColor` — the print/export mode. */
  mono?: boolean;
  /** Draw into a surface of this CSS size instead of the canvas element's. */
  size?: { w: number; h: number };
}

/** WebGL renderer: uploads scene geometry once, draws per-layer ranges. */
export class Renderer {
  private gl: WebGLRenderingContext | null = null;
  private program: WebGLProgram | null = null;
  private uM: WebGLUniformLocation | null = null;
  private uAuto: WebGLUniformLocation | null = null;
  private uPoint: WebGLUniformLocation | null = null;
  private uMono: WebGLUniformLocation | null = null;
  private aPos = 0;
  private aCol = 0;
  private buffers = new Map<number, LayoutBuffers>();
  private uploadedScene: Scene | null = null;
  private canvas: HTMLCanvasElement | null = null;
  /** True while the GPU has dropped the context (sleep/wake, driver reset). */
  contextLost = false;
  /** Fired on loss and on restore so the view can redraw / show a notice. */
  onContextChange: (() => void) | null = null;

  private onLost = (e: Event) => {
    // preventDefault is what allows the browser to hand it back.
    e.preventDefault();
    this.contextLost = true;
    this.gl = null;
    this.onContextChange?.();
  };

  private onRestored = () => {
    const canvas = this.canvas;
    const scene = this.uploadedScene;
    if (!canvas) return;
    this.contextLost = false;
    // Everything GL-side is gone: rebuild the program and re-upload.
    this.program = null;
    this.buffers.clear();
    this.uploadedScene = null;
    if (this.init(canvas) && scene) this.uploadScene(scene);
    this.onContextChange?.();
  };

  init(canvas: HTMLCanvasElement): boolean {
    const gl = canvas.getContext("webgl", {
      antialias: true,
      alpha: false,
      powerPreference: "high-performance",
    });
    if (!gl) return false;
    this.gl = gl;
    this.canvas = canvas;
    canvas.addEventListener("webglcontextlost", this.onLost);
    canvas.addEventListener("webglcontextrestored", this.onRestored);

    const compile = (type: number, src: string) => {
      const sh = gl.createShader(type)!;
      gl.shaderSource(sh, src);
      gl.compileShader(sh);
      if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
        throw new Error(gl.getShaderInfoLog(sh) ?? "shader compile failed");
      }
      return sh;
    };
    const prog = gl.createProgram()!;
    gl.attachShader(prog, compile(gl.VERTEX_SHADER, VS));
    gl.attachShader(prog, compile(gl.FRAGMENT_SHADER, FS));
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
      throw new Error(gl.getProgramInfoLog(prog) ?? "program link failed");
    }
    this.program = prog;
    gl.useProgram(prog);
    this.uM = gl.getUniformLocation(prog, "u_m");
    this.uAuto = gl.getUniformLocation(prog, "u_auto");
    this.uPoint = gl.getUniformLocation(prog, "u_point");
    this.uMono = gl.getUniformLocation(prog, "u_mono");
    this.aPos = gl.getAttribLocation(prog, "a_pos");
    this.aCol = gl.getAttribLocation(prog, "a_col");
    gl.enableVertexAttribArray(this.aPos);
    gl.enableVertexAttribArray(this.aCol);
    return true;
  }

  dispose() {
    for (const b of this.buffers.values()) {
      this.gl?.deleteBuffer(b.lines);
      this.gl?.deleteBuffer(b.points);
      this.gl?.deleteBuffer(b.tris);
      this.gl?.deleteBuffer(b.masks);
    }
    this.buffers.clear();
    this.uploadedScene = null;
    if (this.canvas) {
      this.canvas.removeEventListener("webglcontextlost", this.onLost);
      this.canvas.removeEventListener("webglcontextrestored", this.onRestored);
    }
    if (this.gl && this.program) this.gl.deleteProgram(this.program);
    this.program = null;
  }

  /** Paint the canvas with just the background (no drawing open). */
  clear(canvas: HTMLCanvasElement, background: [number, number, number], dpr: number) {
    const gl = this.gl;
    if (!gl) return;
    const w = Math.max(1, Math.round(canvas.clientWidth * dpr));
    const h = Math.max(1, Math.round(canvas.clientHeight * dpr));
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w;
      canvas.height = h;
    }
    gl.viewport(0, 0, w, h);
    const [br, bg, bb] = background;
    gl.clearColor(br / 255, bg / 255, bb / 255, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
  }

  /** Uploads every layout's geometry to the GPU (idempotent per scene). */
  uploadScene(scene: Scene) {
    const gl = this.gl;
    if (!gl) return;
    if (this.uploadedScene === scene) return;
    // Drop only the old geometry buffers.  A full `dispose()` here would also
    // delete the shader program and detach the context-loss listeners,
    // leaving the renderer permanently unable to draw (blank canvas).
    for (const b of this.buffers.values()) {
      gl.deleteBuffer(b.lines);
      gl.deleteBuffer(b.points);
      gl.deleteBuffer(b.tris);
      gl.deleteBuffer(b.masks);
    }
    this.buffers.clear();
    this.uploadedScene = scene;
    const bytes = new Uint8Array(scene.buffer);
    const base = scene.geometryOffset;
    scene.meta.layouts.forEach((layout, i) => {
      const mk = (): LayoutBuffers => ({
        lines: gl.createBuffer(),
        points: gl.createBuffer(),
        tris: gl.createBuffer(),
        masks: gl.createBuffer(),
      });
      const bufs = mk();
      const upload = (buf: WebGLBuffer | null, offset: number, len: number) => {
        if (!buf || len === 0) return;
        gl.bindBuffer(gl.ARRAY_BUFFER, buf);
        gl.bufferData(
          gl.ARRAY_BUFFER,
          bytes.subarray(base + offset, base + offset + len),
          gl.STATIC_DRAW,
        );
      };
      upload(bufs.lines, layout.lines_offset, layout.lines_len);
      upload(bufs.points, layout.points_offset, layout.points_len);
      upload(bufs.tris, layout.tris_offset, layout.tris_len);
      upload(bufs.masks, layout.masks_offset, layout.masks_len);
      this.buffers.set(i, bufs);
    });
  }

  draw(canvas: HTMLCanvasElement, cam: Camera, layoutIdx: number, opts: DrawOptions) {
    const gl = this.gl;
    if (!gl || !this.program) return;

    const w = Math.max(1, Math.round((opts.size?.w ?? canvas.clientWidth) * opts.dpr));
    const h = Math.max(1, Math.round((opts.size?.h ?? canvas.clientHeight) * opts.dpr));
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w;
      canvas.height = h;
    }
    gl.viewport(0, 0, w, h);
    const [br, bg, bb] = opts.background;
    gl.clearColor(br / 255, bg / 255, bb / 255, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.disable(gl.BLEND);
    gl.disable(gl.DEPTH_TEST);

    gl.useProgram(this.program);
    gl.uniformMatrix3fv(this.uM, false, cam.matrix());
    const [ar, ag, ab] = opts.autoColor;
    gl.uniform3f(this.uAuto, ar / 255, ag / 255, ab / 255);
    gl.uniform1f(this.uMono, opts.mono ? 1 : 0);

    const layout = this.uploadedScene?.meta.layouts[layoutIdx];
    const bufs = this.buffers.get(layoutIdx);
    if (!layout || !bufs) return;

    const bind = (buf: WebGLBuffer | null) => {
      gl.bindBuffer(gl.ARRAY_BUFFER, buf);
      gl.vertexAttribPointer(this.aPos, 2, gl.FLOAT, false, STRIDE, 0);
      gl.vertexAttribPointer(this.aCol, 4, gl.UNSIGNED_BYTE, true, STRIDE, 8);
    };
    const hidden = opts.hidden;
    // Offsets and counts must be whole vertices: a range length that is not a
    // multiple of the stride would make WebGL reject the call with
    // INVALID_OPERATION and silently drop the batch.  `align` keeps triangle
    // and line batches from ending mid-primitive.
    const drawRange = (mode: number, r: { offset: number; len: number }, align = 1) => {
      const count = Math.floor(r.len / STRIDE / align) * align;
      if (count <= 0) return;
      gl.drawArrays(mode, Math.floor(r.offset / STRIDE), count);
    };

    // Fills first (they are the background of a drawing), then hairlines,
    // then points.
    // Buffer data is already the layout's sub-region, so the drawArrays
    // offset is relative to that sub-buffer — do NOT add layout.*_offset.
    if (layout.tris_len > 0) {
      gl.uniform1f(this.uPoint, 1);
      bind(bufs.tris);
      for (const r of layout.tri_ranges) {
        if (hidden.has(r.layer)) continue;
        drawRange(gl.TRIANGLES, r, 3);
      }
    }
    if (layout.lines_len > 0) {
      gl.uniform1f(this.uPoint, 1);
      bind(bufs.lines);
      for (const r of layout.line_ranges) {
        if (hidden.has(r.layer)) continue;
        drawRange(gl.LINES, r, 2);
      }
    }
    if (layout.points_len > 0) {
      bind(bufs.points);
      gl.uniform1f(this.uPoint, Math.max(2, Math.round(2.5 * opts.dpr)));
      for (const r of layout.point_ranges) {
        if (hidden.has(r.layer)) continue;
        drawRange(gl.POINTS, r);
      }
    }
    // WIPEOUT masks go last and use the background colour: their whole job is
    // to hide whatever was drawn before them.
    if (layout.masks_len > 0) {
      gl.uniform1f(this.uPoint, 1);
      gl.uniform3f(this.uAuto, br / 255, bg / 255, bb / 255);
      bind(bufs.masks);
      for (const r of layout.mask_ranges) {
        if (hidden.has(r.layer)) continue;
        drawRange(gl.TRIANGLES, r, 3);
      }
    }
  }
}

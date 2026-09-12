/** 2D orthographic camera: world (Y up, CAD space) ↔ screen (Y down, CSS px). */
export class Camera {
  /** World coordinates of the viewport center. */
  cx = 0;
  cy = 0;
  /** Screen pixels per world unit. */
  scale = 1;
  vw = 1;
  vh = 1;
  /** Scale corresponding to the last fit — reference for zoom %. */
  fitScale = 1;

  setViewport(vw: number, vh: number) {
    this.vw = Math.max(1, vw);
    this.vh = Math.max(1, vh);
  }

  worldFromScreen(sx: number, sy: number): { x: number; y: number } {
    return {
      x: (sx - this.vw / 2) / this.scale + this.cx,
      y: -(sy - this.vh / 2) / this.scale + this.cy,
    };
  }

  screenFromWorld(x: number, y: number): { sx: number; sy: number } {
    return {
      sx: (x - this.cx) * this.scale + this.vw / 2,
      sy: -(y - this.cy) * this.scale + this.vh / 2,
    };
  }

  panByPixels(dx: number, dy: number) {
    this.cx -= dx / this.scale;
    this.cy += dy / this.scale;
  }

  zoomAt(sx: number, sy: number, factor: number) {
    const w = this.worldFromScreen(sx, sy);
    this.scale = Math.min(Math.max(this.scale * factor, 1e-9), 1e9);
    this.cx = w.x - (sx - this.vw / 2) / this.scale;
    this.cy = w.y + (sy - this.vh / 2) / this.scale;
  }

  fit(minx: number, miny: number, maxx: number, maxy: number) {
    if (![minx, miny, maxx, maxy].every((v) => Number.isFinite(v))) {
      this.cx = 0;
      this.cy = 0;
      this.scale = 1;
      this.fitScale = 1;
      return;
    }
    // Degenerate extents (a single point, everything coincident): fall back to
    // a small box around the data — never to the origin, which would render a
    // blank canvas for any drawing stored in survey coordinates.
    const spanX = maxx - minx;
    const spanY = maxy - miny;
    // Only pad the span when it is actually zero: unconditionally mixing in
    // `|min+max| * 1e-3` made small drawings at large coordinates fit to a box
    // a hundred times their size, i.e. look empty.
    const w = spanX > 0 ? spanX : Math.max(Math.abs(maxx) * 1e-3, 1e-6);
    const h = spanY > 0 ? spanY : Math.max(Math.abs(maxy) * 1e-3, 1e-6);
    this.scale = Math.min((this.vw / w) * 0.92, (this.vh / h) * 0.92);
    this.scale = Math.min(Math.max(this.scale, 1e-9), 1e9);
    this.cx = (minx + maxx) / 2;
    this.cy = (miny + maxy) / 2;
    this.fitScale = this.scale;
  }

  /** Column-major mat3 mapping world → clip space for the shader. */
  matrix(): Float32Array {
    const sx = (2 * this.scale) / this.vw;
    const sy = (2 * this.scale) / this.vh;
    return new Float32Array([
      sx, 0, 0,
      0, sy, 0,
      -this.cx * sx, -this.cy * sy, 1,
    ]);
  }
}

/**
 * One-shot tripwire for the projection: the view centre must land on the clip
 * origin.  A drawing whose coordinates sit far from (0,0) — i.e. every real CAD
 * file — renders completely off-screen if this factor is ever wrong, and the
 * text overlay (which uses screenFromWorld) would still look fine.
 */
let projectionChecked = false;
export function checkProjection(cam: Camera): void {
  if (projectionChecked) return;
  projectionChecked = true;
  const m = cam.matrix();
  const x = m[0] * cam.cx + m[3] * cam.cy + m[6];
  const y = m[1] * cam.cx + m[4] * cam.cy + m[7];
  if (Math.abs(x) > 1e-3 || Math.abs(y) > 1e-3) {
    console.error("[camera] projection is off-centre — geometry will not be visible", { x, y });
  }
}

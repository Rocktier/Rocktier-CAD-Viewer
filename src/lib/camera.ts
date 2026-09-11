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
    if (!(maxx > minx) && !(maxy > miny)) {
      // Empty extents: reset to a sane default view.
      this.cx = 0;
      this.cy = 0;
      this.scale = 1;
      this.fitScale = 1;
      return;
    }
    // Guard against zero-area extents (e.g. a single point).
    const w = Math.max(maxx - minx, Math.abs(maxx + minx) * 1e-3, 1e-6);
    const h = Math.max(maxy - miny, Math.abs(maxy + miny) * 1e-3, 1e-6);
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
      -2 * this.cx * sx, -2 * this.cy * sy, 1,
    ]);
  }
}

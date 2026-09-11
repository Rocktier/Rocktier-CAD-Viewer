<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { app } from "../lib/state.svelte";
  import { Renderer } from "../lib/renderer";
  import { Camera } from "../lib/camera";
  import { drawTexts, drawMeasure } from "../lib/text";
  import { formatDist } from "../lib/format";

  let wrap: HTMLDivElement;
  let glCanvas: HTMLCanvasElement;
  let uiCanvas: HTMLCanvasElement;

  const cam = new Camera();
  const renderer = new Renderer();
  let glOk = $state(true);
  let glError = $state("");

  let raf = 0;
  let ro: ResizeObserver | null = null;
  let fitRequested = false;
  let pending: { x: number; y: number } | null = null;
  let cursorWorld: { x: number; y: number } | null = null;
  let dragging = false;
  let panning = $state(false);
  let last: { sx: number; sy: number } | null = null;

  const dpr = () => Math.min(window.devicePixelRatio || 1, 2.5);

  function themeColors() {
    return app.theme === "dark"
      ? { bg: [10, 10, 10] as [number, number, number], auto: [255, 255, 255] as [number, number, number], autoCss: "rgba(255,255,255,0.95)", red: "#FF4A3D" }
      : { bg: [255, 255, 255] as [number, number, number], auto: [26, 26, 26] as [number, number, number], autoCss: "rgba(0,0,0,0.92)", red: "#E64537" };
  }

  function draw() {
    raf = 0;
    if (fitRequested) {
      fitRequested = false;
      const lay = app.scene?.meta.layouts[app.activeLayout];
      if (lay) {
        cam.fit(lay.min_x, lay.min_y, lay.max_x, lay.max_y);
        app.zoomPct = 100;
      }
    }
    if (!app.scene) {
      // Clear canvases when no drawing is open.
      const ctx = uiCanvas?.getContext("2d");
      if (ctx) {
        ctx.setTransform(1, 0, 0, 1, 0, 0);
        ctx.clearRect(0, 0, uiCanvas.width, uiCanvas.height);
      }
      return;
    }
    const th = themeColors();
    if (glOk) {
      renderer.draw(glCanvas, cam, app.activeLayout, {
        autoColor: th.auto,
        background: th.bg,
        hidden: app.hidden,
        dpr: dpr(),
      });
    }
    const ctx = uiCanvas.getContext("2d");
    if (ctx) {
      drawTexts(ctx, uiCanvas, app.scene, app.activeLayout, cam, {
        autoColor: th.autoCss,
        hidden: app.hidden,
        dpr: dpr(),
      });
      if (app.tool === "measure") {
        drawMeasure(
          ctx, uiCanvas, cam,
          pending,
          app.measureResult,
          cursorWorld,
          dpr(),
          th.red,
          app.measureResult ? formatDist(app.measureResult.dist) : null,
        );
      }
    }
  }

  function requestDraw() {
    if (!raf) raf = requestAnimationFrame(draw);
  }

  // React to any relevant state change.
  $effect(() => {
    void app.scene;
    void app.activeLayout;
    void app.theme;
    void app.tool;
    void app.measureResult;
    void app.hidden.size;
    requestDraw();
  });

  // Upload new scenes to the GPU.
  $effect(() => {
    const s = app.scene;
    if (s) {
      renderer.uploadScene(s);
      fitRequested = true;
      requestDraw();
    }
  });

  // Refit when switching layouts.
  $effect(() => {
    void app.activeLayout;
    if (app.scene) {
      fitRequested = true;
      requestDraw();
    }
  });

  // External fit requests (toolbar / F key).
  $effect(() => {
    void app.fitTick;
    if (app.scene) {
      fitRequested = true;
      requestDraw();
    }
  });

  // Reset measure state when the tool changes.
  $effect(() => {
    void app.tool;
    pending = null;
    if (app.tool !== "measure") {
      app.measureResult = null;
    }
    requestDraw();
  });

  function updateCoords(e: MouseEvent) {
    const w = cam.worldFromScreen(e.offsetX, e.offsetY);
    app.coords = { x: w.x, y: w.y };
    cursorWorld = w;
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    // Sensitivity raised so a single wheel notch (deltaY ≈ ±100) moves ~30%.
    // Trackpad micro-events (deltaY ≈ ±2-5) still feel responsive.
    let factor = Math.exp(-e.deltaY * 0.0032);
    if (e.deltaY === 0) factor = e.deltaX < 0 ? 1.15 : 0.85;
    factor = Math.min(Math.max(factor, 0.25), 4);
    cam.zoomAt(e.offsetX, e.offsetY, factor);
    app.zoomPct = Math.round((cam.scale / cam.fitScale) * 100);
    requestDraw();
  }

  function onDown(e: MouseEvent) {
    if (app.tool === "measure" && e.button === 0) {
      const w = cam.worldFromScreen(e.offsetX, e.offsetY);
      if (!pending) {
        pending = w;
        app.measureResult = null;
      } else {
        const dist = Math.hypot(w.x - pending.x, w.y - pending.y);
        app.measureResult = { x1: pending.x, y1: pending.y, x2: w.x, y2: w.y, dist };
        pending = null;
      }
      requestDraw();
      return;
    }
    if (e.button === 0 || e.button === 1 || e.button === 2) {
      dragging = true;
      panning = true;
      last = { sx: e.clientX, sy: e.clientY };
    }
  }

  function onMove(e: MouseEvent) {
    updateCoords(e);
    if (dragging && last) {
      cam.panByPixels(e.clientX - last.sx, e.clientY - last.sy);
      last = { sx: e.clientX, sy: e.clientY };
      requestDraw();
    } else if (app.tool === "measure" && pending) {
      requestDraw();
    }
  }

  function onUp() {
    dragging = false;
    panning = false;
  }

  function onLeave() {
    cursorWorld = null;
  }

  function onContext(e: MouseEvent) {
    e.preventDefault();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      pending = null;
      app.measureResult = null;
      app.tool = "pan";
      requestDraw();
    } else if (e.key === "f" || e.key === "F") {
      if (app.scene) {
        fitRequested = true;
        requestDraw();
      }
    }
  }

  onMount(() => {
    try {
      glOk = renderer.init(glCanvas);
      if (!glOk) glError = "WebGL 初始化失败 / WebGL init failed";
    } catch (err) {
      glOk = false;
      glError = String(err);
    }
    cam.setViewport(wrap.clientWidth, wrap.clientHeight);
    ro = new ResizeObserver(() => {
      cam.setViewport(wrap.clientWidth, wrap.clientHeight);
      requestDraw();
    });
    ro.observe(wrap);
  });

  onDestroy(() => {
    ro?.disconnect();
    if (raf) cancelAnimationFrame(raf);
    renderer.dispose();
  });
</script>

<svelte:window onkeydown={onKey} onmouseup={onUp} />

<div class="canvas-stack" bind:this={wrap}>
  <canvas
    class="gl"
    class:panning
    bind:this={glCanvas}
    onwheel={onWheel}
    onmousedown={onDown}
    onmousemove={onMove}
    onmouseleave={onLeave}
    oncontextmenu={onContext}
  ></canvas>
  <canvas bind:this={uiCanvas} style="pointer-events:none"></canvas>
  {#if !glOk}
    <div class="overlay">
      <div class="overlay-card">
        <div class="overlay-title">WebGL</div>
        <div class="overlay-msg">{glError || "unavailable"}</div>
      </div>
    </div>
  {/if}
</div>

<style>
  .canvas-stack {
    position: absolute;
    inset: 0;
  }
  .canvas-stack canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    display: block;
  }
  canvas.gl {
    cursor: grab;
  }
  canvas.gl.panning {
    cursor: grabbing;
  }
</style>

<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { app } from "../lib/state.svelte";
  import { Renderer } from "../lib/renderer";
  import { Camera, checkProjection } from "../lib/camera";
  import { drawTexts, drawMeasure, syncCanvasSize } from "../lib/text";
  import { formatDist } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import { buildSnapIndex, nearestSnap, SNAP_PX, type SnapHit, type SnapIndex } from "../lib/snap";

  let wrap: HTMLDivElement;
  let glCanvas: HTMLCanvasElement;
  let uiCanvas: HTMLCanvasElement;

  const cam = new Camera();
  const renderer = new Renderer();
  let glOk = $state(true);
  let glError = $state("");
  /** Set while the GPU context is gone; cleared when the renderer rebuilds. */
  let ctxLost = $state(false);
  const glUsable = $derived(glOk && !ctxLost);

  let raf = 0;
  let ro: ResizeObserver | null = null;
  /** Pending fit: false = none, otherwise which box to use. */
  let fitRequested: false | "all" | "core" = false;
  let pending: { x: number; y: number } | null = null;
  let cursorWorld: { x: number; y: number } | null = null;
  let dragging = false;
  let panning = $state(false);
  let last: { sx: number; sy: number } | null = null;
  let snapIndex: SnapIndex = buildSnapIndex(null, 0, new Set());
  let snapHit: SnapHit | null = null;
  /** Where the measure drag started on screen — tells a drag from a click. */
  let dragOrigin: { sx: number; sy: number } | null = null;
  let dragMoved = false;

  const dpr = () => Math.min(window.devicePixelRatio || 1, 2.5);

  // Dark canvas only (CAD convention): ACI 7 renders white, background is
  // near-black.  Coloured entities keep their file colours — yellow dimension
  // text etc. stays readable, unlike on a white canvas.
  function themeColors() {
    return { bg: [10, 10, 10] as [number, number, number], auto: [255, 255, 255] as [number, number, number], autoCss: "rgba(255,255,255,0.95)", red: "#FF4A3D" };
  }

  function draw() {
    raf = 0;
    if (fitRequested) {
      const mode = fitRequested;
      fitRequested = false;
      const lay = app.scene?.meta.layouts[app.activeLayout];
      if (lay) {
        // "core" skips the far-flung outliers (a legend parked off the sheet,
        // a coordinate blip) that would otherwise shrink the whole plan.
        // Never implicit: the user asks for it.
        if (mode === "core") {
          cam.fit(lay.core_min_x, lay.core_min_y, lay.core_max_x, lay.core_max_y);
        } else {
          cam.fit(lay.min_x, lay.min_y, lay.max_x, lay.max_y);
        }
        app.zoomPct = 100;
      }
    }
    if (!app.scene) {
      // Clear both canvases when no drawing is open — otherwise the previous
      // drawing stays visible behind the empty state.
      if (glOk) renderer.clear(glCanvas, themeColors().bg, dpr());
      const ctx = uiCanvas?.getContext("2d");
      if (ctx) {
        syncCanvasSize(uiCanvas, dpr());
        ctx.setTransform(1, 0, 0, 1, 0, 0);
        ctx.clearRect(0, 0, uiCanvas.width, uiCanvas.height);
      }
      return;
    }
    // The exporter needs the live view; a plain snapshot assignment keeps the
    // reactivity graph quiet (nothing renders from these numbers).
    app.camera = { cx: cam.cx, cy: cam.cy, scale: cam.scale, vw: cam.vw, vh: cam.vh };
    const th = themeColors();
    if (glUsable) {
      checkProjection(cam);
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
        upright: app.uprightText,
      });
      if (app.tool === "measure") {
        drawMeasure(ctx, cam, {
          pending,
          result: app.measureResult,
          cursor: cursorWorld,
          snap: snapHit,
          dpr: dpr(),
          red: th.red,
          label: app.measureResult
            ? formatDist(app.measureResult.dist)
            : pending && cursorWorld
              ? formatDist(Math.hypot(cursorWorld.x - pending.x, cursorWorld.y - pending.y))
              : null,
          snapLabel: snapHit
            ? t(snapHit.kind === "midpoint" ? "snapMidpoint" : "snapEndpoint")
            : null,
        });
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
      fitRequested = "all";
      requestDraw();
    }
  });

  // Refit when switching layouts.
  $effect(() => {
    void app.activeLayout;
    if (app.scene) {
      fitRequested = "all";
      requestDraw();
    }
  });

  // External fit requests (toolbar / F key).
  $effect(() => {
    void app.fitTick;
    if (app.scene) {
      fitRequested = "all";
      requestDraw();
    }
  });

  // "Fit content": same view, minus the outliers.
  $effect(() => {
    void app.fitCoreTick;
    if (app.scene) {
      fitRequested = "core";
      requestDraw();
    }
  });

  // Reset measure state when the tool changes.
  $effect(() => {
    void app.tool;
    pending = null;
    snapHit = null;
    dragOrigin = null;
    if (app.tool !== "measure") {
      app.measureResult = null;
    }
    requestDraw();
  });

  // Snap candidates follow the scene, the active layout and layer visibility.
  $effect(() => {
    void app.scene;
    void app.activeLayout;
    void app.hidden.size;
    snapIndex = buildSnapIndex(app.scene, app.activeLayout, app.hidden);
  });

  /** Pointer position in CSS pixels relative to the canvas, whatever the target. */
  function canvasPoint(e: MouseEvent) {
    const r = glCanvas.getBoundingClientRect();
    return { sx: e.clientX - r.left, sy: e.clientY - r.top };
  }

  /** Cursor in world units, snapped to nearby geometry unless Shift bypasses it. */
  function cursorAt(e: MouseEvent) {
    const p = canvasPoint(e);
    const w = cam.worldFromScreen(p.sx, p.sy);
    snapHit =
      app.tool === "measure" && !e.shiftKey
        ? nearestSnap(snapIndex, w.x, w.y, SNAP_PX / cam.scale)
        : null;
    cursorWorld = snapHit ?? w;
    app.coords = cursorWorld;
    return { p, w: cursorWorld };
  }

  function commitMeasure(end: { x: number; y: number }) {
    if (!pending) return;
    app.measureResult = {
      x1: pending.x,
      y1: pending.y,
      x2: end.x,
      y2: end.y,
      dist: Math.hypot(end.x - pending.x, end.y - pending.y),
    };
    pending = null;
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    // Normalise deltaMode: Firefox reports lines (≈3) and some Windows drivers
    // report pages (≈100), both of which would make one notch nearly a no-op
    // (or a teleport) against a factor tuned for pixels.
    const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? 100 : 1;
    const dy = e.deltaY * unit;
    // Sensitivity raised so a single wheel notch (deltaY ≈ ±100) moves ~30%.
    // Trackpad micro-events (deltaY ≈ ±2-5) still feel responsive.
    let factor = dy === 0 ? (e.deltaX < 0 ? 1.15 : 0.85) : Math.exp(-dy * 0.0032);
    factor = Math.min(Math.max(factor, 0.25), 4);
    const p = canvasPoint(e);
    cam.zoomAt(p.sx, p.sy, factor);
    app.zoomPct = Math.min(99999, Math.max(1, Math.round((cam.scale / cam.fitScale) * 100)));
    requestDraw();
  }

  function onDown(e: MouseEvent) {
    if (app.tool === "measure" && e.button === 0) {
      const { p, w } = cursorAt(e);
      if (!pending) {
        // First point: remember where the press started so a drag can complete
        // the measurement on release, while a plain click keeps it pending for
        // the click-click workflow.
        pending = w;
        app.measureResult = null;
        dragOrigin = p;
        dragMoved = false;
      } else {
        commitMeasure(w);
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
    const { p } = cursorAt(e);
    if (dragOrigin && Math.hypot(p.sx - dragOrigin.sx, p.sy - dragOrigin.sy) > 3) {
      dragMoved = true;
    }
    if (dragging && last) {
      cam.panByPixels(e.clientX - last.sx, e.clientY - last.sy);
      last = { sx: e.clientX, sy: e.clientY };
      requestDraw();
    } else if (app.tool === "measure") {
      requestDraw();
    }
  }

  function onUp(e: MouseEvent) {
    const origin = dragOrigin;
    dragOrigin = null;
    if (origin && dragMoved && pending) {
      // Drag-to-measure: releasing with the pointer moved commits the distance.
      commitMeasure(cursorAt(e).w);
      requestDraw();
    }
    dragging = false;
    panning = false;
  }

  function onLeave() {
    cursorWorld = null;
    snapHit = null;
    app.coords = { x: 0, y: 0 };
  }

  function onContext(e: MouseEvent) {
    e.preventDefault();
  }

  function onKey(e: KeyboardEvent) {
    // Don't hijack typing (the layer search box) or browser shortcuts.
    if (e.isComposing || e.metaKey || e.ctrlKey || e.altKey) return;
    const el = e.target as HTMLElement | null;
    if (el && /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName)) return;

    if (e.key === "Escape") {
      if (app.aboutOpen) {
        app.aboutOpen = false;
        return;
      }
      pending = null;
      dragOrigin = null;
      app.measureResult = null;
      app.tool = "pan";
      requestDraw();
    } else if (e.key === "f" || e.key === "F") {
      if (app.scene) {
        fitRequested = "all";
        requestDraw();
      }
    } else if (e.key === "c" || e.key === "C") {
      if (app.scene) {
        fitRequested = "core";
        requestDraw();
      }
    }
  }

  let dprMq: MediaQueryList | null = null;

  /** Canvas backing stores are only re-sized inside `draw()`, so dragging the
   *  window to a display with a different scale needs an explicit nudge. */
  function watchDpr() {
    dprMq?.removeEventListener("change", onDprChange);
    dprMq = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
    dprMq.addEventListener("change", onDprChange);
  }

  function onDprChange() {
    cam.setViewport(wrap.clientWidth, wrap.clientHeight);
    requestDraw();
    watchDpr();
  }

  onMount(() => {
    try {
      glOk = renderer.init(glCanvas);
      if (!glOk) glError = "WebGL 初始化失败 / WebGL init failed";
    } catch (err) {
      glOk = false;
      glError = String(err);
    }
    renderer.onContextChange = () => {
      ctxLost = renderer.contextLost;
      requestDraw();
    };
    cam.setViewport(wrap.clientWidth, wrap.clientHeight);
    ro = new ResizeObserver(() => {
      cam.setViewport(wrap.clientWidth, wrap.clientHeight);
      requestDraw();
    });
    ro.observe(wrap);
    watchDpr();
  });

  onDestroy(() => {
    ro?.disconnect();
    dprMq?.removeEventListener("change", onDprChange);
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
  {#if !glUsable}
    <div class="overlay">
      <div class="overlay-card">
        <div class="overlay-title">WebGL</div>
        <div class="overlay-msg">{glError || (ctxLost ? "GPU 上下文丢失，正在恢复…" : "unavailable")}</div>
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

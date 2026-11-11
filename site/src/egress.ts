// The egress scene: requests leave agent sandboxes as pixels, meet Hyperbeam, and either pass through to the
// system they asked for or break apart at the gate. The cards are HTML laid out by CSS; two canvases draw the
// wires and packets behind them, and the impacts, debris and labels in front. Plays only while on screen.

import { AGENTS, AMBIENT, BEATS, clock, logRow, type AgentId, type Beat, type DestId, type Lane, type Request } from "./egress-data";

type Pt = { x: number; y: number };
type Colors = Record<"primary" | "second" | "core" | "run" | "fail" | "wait" | "wire" | "surface" | "foreground", string>;

const SPEED = 330; // px per second
const BEAT_EVERY = 1.45; // seconds
const PIXEL = 6;

/** A polyline walked at constant speed. `impact` and `exit` are the distances at which it meets and leaves the gate. */
class Route {
  cum: number[] = [0];
  constructor(public pts: Pt[], public impact: number, public exit: number) {
    for (let i = 1; i < pts.length; i++) this.cum.push(this.cum[i - 1] + Math.hypot(pts[i].x - pts[i - 1].x, pts[i].y - pts[i - 1].y));
  }
  get length() { return this.cum[this.cum.length - 1]; }
  at(s: number): Pt {
    s = Math.max(0, Math.min(s, this.length));
    let i = 1;
    while (i < this.cum.length - 1 && this.cum[i] < s) i++;
    const span = this.cum[i] - this.cum[i - 1] || 1;
    const t = (s - this.cum[i - 1]) / span;
    const a = this.pts[i - 1], b = this.pts[i];
    return { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t };
  }
}

type Packet = { route: Route; s: number; ok: boolean; small: boolean; beat?: Request; crossed: boolean; lane: Lane; dest: DestId; agent: AgentId };
type Particle = { x: number; y: number; vx: number; vy: number; born: number; life: number; color: string; size: number };
type Ripple = { x: number; y: number; born: number; color: string; vertical: boolean };
type Beam = { route: Route; end: number; ok: boolean; died: number };
type Label = { text: string; x: number; y: number; align: CanvasTextAlign; color: string; born: number };

export function startEgressScene(root: HTMLElement) {
  const back = root.querySelector<HTMLCanvasElement>("[data-egress-back]")!;
  const front = root.querySelector<HTMLCanvasElement>("[data-egress-front]")!;
  const log = root.querySelector<HTMLOListElement>("[data-egress-log]")!;
  const gate = root.querySelector<HTMLElement>("[data-gate]")!;
  const agentsCol = root.querySelector<HTMLElement>("[data-agents]")!;
  const bctx = back.getContext("2d")!;
  const fctx = front.getContext("2d")!;
  const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;

  let colors = readColors();
  let dpr = 1, width = 0, height = 0, horizontal = true;
  let routes = new Map<string, Route>();
  let wires: Pt[][] = [];
  let wall: { a: Pt; b: Pt; c: Pt; d: Pt } | null = null;
  let lanePts = new Map<Lane, { impact: Pt; exit: Pt }>();

  let packets: Packet[] = [];
  let particles: Particle[] = [];
  let ripples: Ripple[] = [];
  let labels: Label[] = [];
  let beams: Beam[] = [];
  let time = 0, nextBeat = 0.4, nextAmbient = 0.2, beatIndex = 0, logSeconds = 14; // carries on from the static rows' clock

  function readColors(): Colors {
    const s = getComputedStyle(document.documentElement);
    const v = (n: string) => s.getPropertyValue(n).trim();
    return { primary: v("--primary"), second: v("--second"), core: v("--core"), run: v("--run"), fail: v("--fail"), wait: v("--wait"), wire: v("--border-strong"), surface: v("--surface"), foreground: v("--foreground") };
  }

  const rel = (el: Element) => {
    const r = el.getBoundingClientRect(), o = root.getBoundingClientRect();
    return { l: r.left - o.left, t: r.top - o.top, r: r.right - o.left, b: r.bottom - o.top, cx: (r.left + r.right) / 2 - o.left, cy: (r.top + r.bottom) / 2 - o.top };
  };

  // ------------------------------------------------------------- geometry

  function layout() {
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    width = root.clientWidth;
    height = root.clientHeight;
    for (const c of [back, front]) {
      c.width = Math.round(width * dpr);
      c.height = Math.round(height * dpr);
    }
    const g = rel(gate), ac = rel(agentsCol);
    horizontal = g.l > ac.r - 1;
    const destCol = rel(root.querySelector("[data-dests]")!);
    // The buses run down the middle of each gap; on narrow screens they run across it.
    const busIn = horizontal ? (ac.r + g.l) / 2 : (ac.b + g.t) / 2;
    const busOut = horizontal ? (g.r + destCol.l) / 2 : (g.b + destCol.t) / 2;

    lanePts = new Map();
    for (const el of root.querySelectorAll<HTMLElement>("[data-lane]")) {
      const r = rel(el);
      lanePts.set(el.dataset.lane as Lane, horizontal
        ? { impact: { x: g.l, y: r.cy }, exit: { x: g.r, y: r.cy } }
        : { impact: { x: r.cx, y: g.t }, exit: { x: r.cx, y: g.b } });
    }
    wall = horizontal
      ? { a: { x: g.l, y: g.t + 10 }, b: { x: g.l, y: g.b - 10 }, c: { x: g.r, y: g.t + 10 }, d: { x: g.r, y: g.b - 10 } }
      : { a: { x: g.l + 10, y: g.t }, b: { x: g.r - 10, y: g.t }, c: { x: g.l + 10, y: g.b }, d: { x: g.r - 10, y: g.b } };

    const agentPt = new Map<string, Pt>();
    for (const el of root.querySelectorAll<HTMLElement>("[data-agent]")) {
      const r = rel(el);
      agentPt.set(el.dataset.agent!, horizontal ? { x: r.r, y: r.cy } : { x: r.cx, y: r.b });
    }
    const destPt = new Map<string, Pt>();
    for (const el of root.querySelectorAll<HTMLElement>("[data-dest]")) {
      const r = rel(el);
      destPt.set(el.dataset.dest!, horizontal ? { x: r.l, y: r.cy } : { x: r.cx, y: r.t });
    }

    routes = new Map();
    for (const [agent, a] of agentPt) for (const [lane, { impact, exit }] of lanePts) for (const [dest, d] of destPt) {
      const pts = horizontal
        ? [a, { x: busIn, y: a.y }, { x: busIn, y: impact.y }, impact, exit, { x: busOut, y: exit.y }, { x: busOut, y: d.y }, d]
        : [a, { x: a.x, y: busIn }, { x: impact.x, y: busIn }, impact, exit, { x: exit.x, y: busOut }, { x: d.x, y: busOut }, d];
      const probe = new Route(pts, 0, 0);
      routes.set(`${agent}|${lane}|${dest}`, new Route(pts, probe.cum[3], probe.cum[4]));
    }

    // Faint static wiring: a stub from each card to its bus, the buses, and the lane stubs either side of the gate.
    wires = [];
    const span = (pts: Pt[], key: "x" | "y") => [Math.min(...pts.map((p) => p[key])), Math.max(...pts.map((p) => p[key]))];
    const ins = [...lanePts.values()].map((l) => l.impact), outs = [...lanePts.values()].map((l) => l.exit);
    const as = [...agentPt.values()], ds = [...destPt.values()];
    if (horizontal) {
      for (const a of as) wires.push([a, { x: busIn, y: a.y }]);
      for (const p of ins) wires.push([{ x: busIn, y: p.y }, p]);
      for (const p of outs) wires.push([p, { x: busOut, y: p.y }]);
      for (const d of ds) wires.push([{ x: busOut, y: d.y }, d]);
      const [i0, i1] = span([...as, ...ins], "y"), [o0, o1] = span([...ds, ...outs], "y");
      wires.push([{ x: busIn, y: i0 }, { x: busIn, y: i1 }], [{ x: busOut, y: o0 }, { x: busOut, y: o1 }]);
    } else {
      for (const a of as) wires.push([a, { x: a.x, y: busIn }]);
      for (const p of ins) wires.push([{ x: p.x, y: busIn }, p]);
      for (const p of outs) wires.push([p, { x: p.x, y: busOut }]);
      for (const d of ds) wires.push([{ x: d.x, y: busOut }, d]);
      const [i0, i1] = span([...as, ...ins], "x"), [o0, o1] = span([...ds, ...outs], "x");
      wires.push([{ x: i0, y: busIn }, { x: i1, y: busIn }], [{ x: o0, y: busOut }, { x: o1, y: busOut }]);
    }
    packets = [];
    beams = [];
    draw();
  }

  // ----------------------------------------------------------- behaviour

  const flash = (el: Element | null, cls: string, ms: number) => {
    if (!el) return;
    el.classList.remove(cls);
    void (el as HTMLElement).offsetWidth;
    el.classList.add(cls);
    setTimeout(() => el.classList.remove(cls), ms);
  };

  function addLog(beat: Beat) {
    log.insertAdjacentHTML("afterbegin", logRow(beat, clock(logSeconds)));
    log.firstElementChild?.classList.add("elog-new");
    while (log.children.length > 5) log.lastElementChild!.remove();
  }

  function play(beat: Beat) {
    logSeconds += 1 + Math.floor(Math.random() * 4);
    if (beat.kind !== "request") {
      const chips = root.querySelectorAll(`[data-owner="${beat.owner}"] [data-grant="${beat.grant}"]`);
      chips.forEach((c) => c.classList.toggle("revoked", beat.kind === "revoke"));
      root.querySelectorAll(`[data-agent][data-owner="${beat.owner}"]`).forEach((c) => flash(c, beat.kind === "revoke" ? "is-revoking" : "is-granted", 1200));
      addLog(beat);
      return;
    }
    launch(beat.agent, beat.lane, beat.dest, beat.ok, false, beat);
  }

  function launch(agent: AgentId, lane: Lane, dest: DestId, ok: boolean, small: boolean, beat?: Request) {
    const route = routes.get(`${agent}|${lane}|${dest}`);
    if (!route) return;
    packets.push({ route, s: 0, ok, small, beat, crossed: false, lane, dest, agent });
    if (!small) flash(root.querySelector(`[data-agent="${agent}"]`), "is-sending", 500);
  }

  function hitGate(p: Packet) {
    const at = lanePts.get(p.lane)!;
    const laneEl = root.querySelector(`[data-lane="${p.lane}"]`);
    if (p.ok) {
      if (p.small) return;
      ripples.push({ ...at.impact, born: time, color: colors.run, vertical: horizontal });
      flash(laneEl, "is-hit", 450);
      labels.push(horizontal
        ? { text: `✓ ${p.beat!.reason}`, x: at.exit.x + 12, y: at.exit.y - 16, align: "left", color: colors.run, born: time }
        : { text: `✓ ${p.beat!.reason}`, x: at.exit.x, y: at.exit.y + 22, align: "center", color: colors.run, born: time });
      addLog(p.beat!);
      return;
    }
    // Refused: the packet breaks into pixels that scatter back the way they came.
    const { x, y } = at.impact;
    for (let i = 0; i < 30; i++) {
      const back = 80 + Math.random() * 240, side = (Math.random() - 0.5) * 300;
      particles.push({
        x, y, born: time, life: 0.5 + Math.random() * 0.6, size: Math.random() < 0.35 ? 6 : Math.random() < 0.5 ? 4 : 3,
        vx: horizontal ? -back : side, vy: horizontal ? side : -back,
        color: Math.random() < 0.7 ? colors.fail : Math.random() < 0.5 ? colors.second : colors.core,
      });
    }
    ripples.push({ x, y, born: time, color: colors.fail, vertical: horizontal });
    flash(laneEl, "is-blocked", 700);
    flash(root.querySelector(`[data-agent="${p.agent}"]`), "is-blocked", 900);
    flash(root.querySelector(`[data-dest="${p.dest}"]`), "is-refused", 900);
    labels.push(horizontal
      ? { text: `✕ ${p.beat!.reason}`, x: x - 12, y: y - 16, align: "right", color: colors.fail, born: time }
      : { text: `✕ ${p.beat!.reason}`, x, y: y - 20, align: "center", color: colors.fail, born: time });
    addLog(p.beat!);
  }

  function step(dt: number) {
    time += dt;
    if (time >= nextBeat) {
      play(BEATS[beatIndex]);
      beatIndex = (beatIndex + 1) % BEATS.length;
      nextBeat = time + BEAT_EVERY;
    }
    if (time >= nextAmbient) {
      const a = AMBIENT[Math.floor(Math.random() * AMBIENT.length)];
      launch(a.agent, a.lane, a.dest, true, true);
      nextAmbient = time + 0.25 + Math.random() * 0.45;
    }
    for (const p of packets) {
      p.s += SPEED * (p.small ? 1.25 : 1) * dt;
      if (!p.crossed && p.s >= p.route.impact) {
        p.crossed = true;
        hitGate(p);
      }
    }
    packets = packets.filter((p) => {
      const done = (!p.ok && p.crossed) || p.s >= p.route.length;
      if (!done) return true;
      if (!p.small) {
        beams.push({ route: p.route, end: Math.min(p.s, p.ok ? p.route.length : p.route.impact), ok: p.ok, died: time });
        if (p.ok) flash(root.querySelector(`[data-dest="${p.dest}"]`), "is-hit", 600);
      }
      return false;
    });
    beams = beams.filter((b) => time - b.died < 0.7);
    for (const q of particles) {
      q.x += q.vx * dt;
      q.y += q.vy * dt;
      q.vx *= 0.94;
      q.vy = q.vy * 0.94 + (horizontal ? 260 * dt : 0);
    }
    particles = particles.filter((q) => time - q.born < q.life);
    ripples = ripples.filter((r) => time - r.born < 0.6);
    labels = labels.filter((l) => time - l.born < 1.7);
  }

  // ---------------------------------------------------------------- paint

  const snap = (v: number) => Math.round(v * dpr) / dpr;
  function px(ctx: CanvasRenderingContext2D, x: number, y: number, size: number) {
    ctx.fillRect(snap(x - size / 2), snap(y - size / 2), size, size);
  }

  /** Strokes a route between two distances along it. */
  function strokeRoute(ctx: CanvasRenderingContext2D, r: Route, from: number, to: number) {
    if (to <= from) return;
    ctx.beginPath();
    const a = r.at(from);
    ctx.moveTo(snap(a.x) + 0.5, snap(a.y) + 0.5);
    for (let i = 1; i < r.pts.length; i++) {
      if (r.cum[i] <= from) continue;
      if (r.cum[i] >= to) break;
      ctx.lineTo(snap(r.pts[i].x) + 0.5, snap(r.pts[i].y) + 0.5);
    }
    const b = r.at(to);
    ctx.lineTo(snap(b.x) + 0.5, snap(b.y) + 0.5);
    ctx.stroke();
  }

  /** The lit path of a request: violet up to the gate, green beyond it. */
  function beam(r: Route, end: number, alpha: number) {
    bctx.lineWidth = 2;
    bctx.globalAlpha = alpha;
    bctx.strokeStyle = colors.primary;
    strokeRoute(bctx, r, 0, Math.min(end, r.impact));
    bctx.strokeStyle = colors.run;
    strokeRoute(bctx, r, r.exit, end);
    bctx.globalAlpha = 1;
  }

  function draw() {
    bctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    fctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    bctx.clearRect(0, 0, width, height);
    fctx.clearRect(0, 0, width, height);

    // Wires.
    bctx.strokeStyle = colors.wire;
    bctx.globalAlpha = 0.9;
    bctx.lineWidth = 1;
    bctx.setLineDash([2, 3]);
    bctx.beginPath();
    for (const [a, b] of wires) {
      bctx.moveTo(snap(a.x) + 0.5, snap(a.y) + 0.5);
      bctx.lineTo(snap(b.x) + 0.5, snap(b.y) + 0.5);
    }
    bctx.stroke();
    bctx.setLineDash([]);
    bctx.globalAlpha = 1;

    for (const b of beams) beam(b.route, b.end, 0.4 * (1 - (time - b.died) / 0.7));
    for (const p of packets) if (!p.small) beam(p.route, p.s, 0.4);

    // Packets with a stepped pixel trail, violet inside the sandbox and green once Hyperbeam has let them out.
    for (const p of packets) {
      const size = p.small ? 4 : PIXEL;
      const trail = p.small ? 4 : 7;
      for (let k = trail; k >= 0; k--) {
        const s = p.s - k * (p.small ? 5 : 6);
        if (s < 0) continue;
        const pt = p.route.at(s);
        bctx.fillStyle = s >= p.route.exit ? colors.run : colors.primary;
        bctx.globalAlpha = (p.small ? 0.45 : 1) * (1 - k / (trail + 1));
        px(bctx, pt.x, pt.y, k === 0 ? size : Math.max(2, size - 2));
      }
      if (!p.small) {
        const head = p.route.at(p.s);
        bctx.globalAlpha = 0.35;
        bctx.fillStyle = p.s >= p.route.exit ? colors.run : colors.primary;
        bctx.beginPath();
        bctx.arc(head.x, head.y, 11, 0, Math.PI * 2);
        bctx.fill();
        bctx.globalAlpha = 1;
        bctx.fillStyle = colors.core;
        px(bctx, head.x, head.y, 2);
      }
    }
    bctx.globalAlpha = 1;

    // The gate's two faces: a pixel beam, gently charged.
    if (wall) {
      const pulse = 0.75 + 0.25 * Math.sin(time * 3);
      fctx.shadowColor = colors.primary;
      fctx.shadowBlur = 14;
      for (const [a, b, alpha] of [[wall.a, wall.b, pulse], [wall.c, wall.d, pulse * 0.45]] as const) {
        const grad = fctx.createLinearGradient(a.x, a.y, b.x, b.y);
        grad.addColorStop(0, colors.primary);
        grad.addColorStop(0.5, colors.second);
        grad.addColorStop(1, colors.primary);
        fctx.fillStyle = grad;
        fctx.globalAlpha = alpha;
        if (horizontal) fctx.fillRect(snap(a.x - 2), a.y, 4, b.y - a.y);
        else fctx.fillRect(a.x, snap(a.y - 2), b.x - a.x, 4);
      }
      fctx.shadowBlur = 0;
      fctx.globalAlpha = 1;
    }

    // Impacts: a flare along the gate's face where the request landed.
    for (const r of ripples) {
      const t = (time - r.born) / 0.6;
      const reach = 10 + 34 * t;
      fctx.globalAlpha = (1 - t) * 0.9;
      fctx.fillStyle = r.color;
      if (r.vertical) fctx.fillRect(snap(r.x - 2), r.y - reach, 4, reach * 2);
      else fctx.fillRect(r.x - reach, snap(r.y - 2), reach * 2, 4);
      fctx.globalAlpha = (1 - t) * 0.25;
      fctx.beginPath();
      fctx.arc(r.x, r.y, 6 + 26 * t, 0, Math.PI * 2);
      fctx.fill();
    }

    for (const q of particles) {
      fctx.globalAlpha = Math.max(0, 1 - (time - q.born) / q.life);
      fctx.fillStyle = q.color;
      px(fctx, q.x, q.y, q.size);
    }

    // Verdict labels.
    fctx.font = `500 11.5px "JetBrains Mono Variable", ui-monospace, monospace`;
    fctx.textBaseline = "middle";
    for (const l of labels) {
      const age = time - l.born;
      const alpha = Math.min(1, age / 0.12) * Math.min(1, (1.7 - age) / 0.35);
      const y = l.y - Math.min(age, 0.4) * 10;
      const w = fctx.measureText(l.text).width + 16;
      const x = Math.max(8, Math.min(width - w - 8, l.align === "right" ? l.x - w : l.align === "center" ? l.x - w / 2 : l.x));
      fctx.globalAlpha = alpha;
      fctx.fillStyle = colors.surface;
      fctx.strokeStyle = l.color;
      fctx.lineWidth = 1;
      fctx.beginPath();
      fctx.roundRect(snap(x) + 0.5, snap(y - 11) + 0.5, Math.round(w), 22, 6);
      fctx.fill();
      fctx.stroke();
      fctx.fillStyle = l.color;
      fctx.textAlign = "left";
      fctx.fillText(l.text, x + 8, y + 0.5);
    }
    fctx.globalAlpha = 1;
  }

  // ------------------------------------------------------------- lifecycle

  let raf = 0, last = 0, visible = false;
  const frame = (now: number) => {
    const dt = Math.min(0.05, (now - last) / 1000 || 0);
    last = now;
    step(dt);
    draw();
    raf = requestAnimationFrame(frame);
  };
  const run = () => {
    if (raf || reduced || !visible || document.hidden) return;
    last = performance.now();
    raf = requestAnimationFrame(frame);
  };
  const stop = () => {
    cancelAnimationFrame(raf);
    raf = 0;
  };

  layout();
  new ResizeObserver(() => layout()).observe(root);
  document.fonts?.ready.then(layout);
  new MutationObserver(() => {
    colors = readColors();
    draw();
  }).observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });
  new IntersectionObserver((entries) => {
    visible = entries.some((e) => e.isIntersecting);
    visible ? run() : stop();
  }).observe(root);
  document.addEventListener("visibilitychange", () => (document.hidden ? stop() : run()));
}

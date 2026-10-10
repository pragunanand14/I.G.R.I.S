import { Hand, MousePointer2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import type { PointerEvent as TvInput } from "@/types/home";

/** Finger/touchpad movement → TV cursor pixels. */
const SPEED = { touch: 2.2, mouse: 1.4 };
/** Send collected movement this often (smooth, without flooding the TV). */
const FLUSH_MS = 30;

/**
 * Sends cursor input to the TV in small batches: movement and scrolling add
 * up between sends, clicks are kept in order. One request at a time.
 */
function useTvPointer(id: string) {
  const pending = useRef({ dx: 0, dy: 0, sx: 0, sy: 0, clicks: 0 });
  const busy = useRef(false);
  const [error, setError] = useState<string | null>(null);

  const flush = useCallback(async () => {
    const p = pending.current;
    if (busy.current || (Math.round(p.dx) === 0 && Math.round(p.dy) === 0 && Math.round(p.sx) === 0 && Math.round(p.sy) === 0 && p.clicks === 0)) return;
    const events: TvInput[] = [];
    const dx = Math.round(p.dx);
    const dy = Math.round(p.dy);
    if (dx || dy) events.push({ type: "move", dx, dy });
    const sx = Math.round(p.sx);
    const sy = Math.round(p.sy);
    if (sx || sy) events.push({ type: "scroll", dx: sx, dy: sy });
    for (let i = 0; i < p.clicks; i++) events.push({ type: "click" });
    pending.current = { dx: p.dx - dx, dy: p.dy - dy, sx: p.sx - sx, sy: p.sy - sy, clicks: 0 };
    busy.current = true;
    try {
      await api.homePointer(id, events);
      setError(null);
    } catch (err) {
      setError(BackendError.from(err).message);
    } finally {
      busy.current = false;
    }
  }, [id]);

  useEffect(() => {
    const t = setInterval(() => void flush(), FLUSH_MS);
    return () => {
      clearInterval(t);
      void api.homePointerStop(id).catch(() => undefined);
    };
  }, [flush, id]);

  return useMemo(
    () => ({
      move: (dx: number, dy: number) => {
        pending.current.dx += dx;
        pending.current.dy += dy;
      },
      scroll: (dx: number, dy: number) => {
        pending.current.sx += dx;
        pending.current.sy += dy;
      },
      click: () => {
        pending.current.clicks += 1;
        void flush();
      },
      error,
    }),
    [flush, error],
  );
}

/**
 * Control the TV's cursor. Phone: slide a finger in the box, tap to click,
 * two fingers to scroll. Laptop: "Use your touchpad" captures the pointer —
 * the laptop's touchpad (or mouse) then moves the TV cursor; Esc gives it back.
 */
export function Touchpad({ id, phone = false }: { id: string; phone?: boolean }) {
  const tv = useTvPointer(id);
  const box = useRef<HTMLDivElement>(null);
  const touches = useRef(new Map<number, { x: number; y: number }>());
  const gesture = useRef({ start: 0, moved: 0, multi: false });
  const [locked, setLocked] = useState(false);
  const [dot, setDot] = useState<{ x: number; y: number; key: number } | null>(null);

  // Laptop: the real touchpad drives the TV while the pointer is captured.
  useEffect(() => {
    const el = box.current;
    const onChange = () => setLocked(document.pointerLockElement === el);
    const onMove = (e: MouseEvent) => {
      if (document.pointerLockElement === el) tv.move(e.movementX * SPEED.mouse, e.movementY * SPEED.mouse);
    };
    const onDown = (e: MouseEvent) => {
      if (document.pointerLockElement === el && e.button === 0) tv.click();
    };
    const onWheel = (e: WheelEvent) => {
      if (document.pointerLockElement !== el) return;
      e.preventDefault();
      tv.scroll(Math.max(-40, Math.min(40, e.deltaX / 4)), Math.max(-40, Math.min(40, e.deltaY / 4)));
    };
    document.addEventListener("pointerlockchange", onChange);
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mousedown", onDown);
    document.addEventListener("wheel", onWheel, { passive: false });
    return () => {
      document.removeEventListener("pointerlockchange", onChange);
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("wheel", onWheel);
      if (document.pointerLockElement === el) document.exitPointerLock();
    };
  }, [tv]);

  // Phone (and dragging with a mouse): the box is the touchpad.
  const down = (e: React.PointerEvent<HTMLDivElement>) => {
    if (locked) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    touches.current.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (touches.current.size === 1) gesture.current = { start: performance.now(), moved: 0, multi: false };
    else gesture.current.multi = true;
    const r = e.currentTarget.getBoundingClientRect();
    setDot({ x: e.clientX - r.left, y: e.clientY - r.top, key: performance.now() });
  };
  const move = (e: React.PointerEvent<HTMLDivElement>) => {
    const last = touches.current.get(e.pointerId);
    if (!last || locked) return;
    const dx = e.clientX - last.x;
    const dy = e.clientY - last.y;
    touches.current.set(e.pointerId, { x: e.clientX, y: e.clientY });
    gesture.current.moved += Math.abs(dx) + Math.abs(dy);
    const speed = e.pointerType === "mouse" ? SPEED.mouse : SPEED.touch;
    if (touches.current.size >= 2) {
      // Two fingers: scroll (half per finger, so both together count once).
      tv.scroll(-dx / 2, -dy / 2);
    } else {
      tv.move(dx * speed, dy * speed);
    }
    const r = e.currentTarget.getBoundingClientRect();
    setDot((d) => ({ x: e.clientX - r.left, y: e.clientY - r.top, key: d?.key ?? 0 }));
  };
  const up = (e: React.PointerEvent<HTMLDivElement>) => {
    touches.current.delete(e.pointerId);
    if (touches.current.size > 0) return;
    const g = gesture.current;
    // A short tap without movement is a click.
    if (!g.multi && g.moved < 8 && performance.now() - g.start < 300) tv.click();
    setDot(null);
  };

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between gap-3">
        <h3 className="text-xs font-medium text-faint">Touchpad</h3>
        {!phone && (
          <button
            type="button"
            onClick={() => (locked ? document.exitPointerLock() : void box.current?.requestPointerLock())}
            className={`flex items-center gap-1.5 rounded-full px-3 py-1.5 text-xs font-medium ${locked ? "bg-fg text-bg" : "bg-surface-strong text-fg hover:bg-surface-hover"}`}
          >
            <MousePointer2 className="size-3.5" /> {locked ? "Stop using your touchpad" : "Use your touchpad"}
          </button>
        )}
      </div>
      <div
        ref={box}
        role="application"
        aria-label="TV touchpad"
        onPointerDown={down}
        onPointerMove={move}
        onPointerUp={up}
        onPointerCancel={up}
        className={`relative grid touch-none place-items-center overflow-hidden rounded-2xl select-none ${phone ? "h-56" : "h-44"} ${
          locked ? "bg-accent/10 ring-2 ring-accent/60" : "bg-surface-strong"
        } transition-[background-color,box-shadow] duration-300`}
        style={{ backgroundImage: "radial-gradient(circle, var(--border-strong) 1px, transparent 1px)", backgroundSize: "18px 18px" }}
      >
        {dot && (
          <span
            key={dot.key}
            aria-hidden="true"
            className="pointer-events-none absolute size-10 -translate-x-1/2 -translate-y-1/2 rounded-full bg-accent/25 ring-1 ring-accent/50"
            style={{ left: dot.x, top: dot.y }}
          />
        )}
        <p className="pointer-events-none flex items-center gap-2 px-4 text-center text-xs text-faint">
          <Hand className="size-4 shrink-0" />
          {locked
            ? "Your touchpad now moves the TV cursor · click to click · two-finger swipe to scroll · Esc to stop"
            : phone
              ? "Slide to move · tap to click · two fingers to scroll"
              : "Drag here to move · click to click — or use your touchpad"}
        </p>
      </div>
      {tv.error && (
        <p role="alert" className="text-xs text-danger">
          {tv.error}
        </p>
      )}
    </div>
  );
}

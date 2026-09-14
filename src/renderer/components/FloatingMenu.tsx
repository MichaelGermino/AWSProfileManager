import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';

/**
 * A dropdown rendered at fixed coordinates through a portal on `document.body`.
 *
 * Why not a plain `absolute` child of the trigger: the profiles card is `overflow-hidden`, so any
 * menu extending past its bottom edge is CLIPPED and unreachable — most visibly on the last folder
 * when there are no ungrouped profiles below it to give the container extra height. `z-index`
 * cannot fix that; only escaping the clipping ancestor can.
 *
 * The menu measures itself on mount and pulls back inside the viewport, so it stays usable no
 * matter where the trigger sits on screen.
 */

export interface FloatingMenuProps {
  /** Preferred position in viewport coordinates. With align 'right', x is the menu's RIGHT edge. */
  x: number;
  y: number;
  align?: 'left' | 'right';
  /**
   * The element that opened the menu. Clicks on it are not treated as "outside", otherwise its own
   * toggle handler would immediately reopen the menu this close just dismissed.
   */
  triggerRef?: React.RefObject<HTMLElement | null>;
  onClose: () => void;
  className?: string;
  children: React.ReactNode;
}

const VIEWPORT_PADDING = 8;

export function FloatingMenu({
  x,
  y,
  align = 'left',
  triggerRef,
  onClose,
  className = '',
  children,
}: FloatingMenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  // Rendered off-screen for one frame so it can be measured before it is placed; without this the
  // menu visibly jumps from the unclamped position to the clamped one.
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    const left = align === 'right' ? x - width : x;
    setPos({
      x: Math.max(VIEWPORT_PADDING, Math.min(left, window.innerWidth - width - VIEWPORT_PADDING)),
      y: Math.max(VIEWPORT_PADDING, Math.min(y, window.innerHeight - height - VIEWPORT_PADDING)),
    });
  }, [x, y, align]);

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (ref.current?.contains(target)) return;
      if (triggerRef?.current?.contains(target)) return;
      onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    // Capture phase, so scrolling ANY ancestor container dismisses it — the menu is positioned in
    // viewport coordinates and would otherwise detach from its trigger. Scrolling the menu's own
    // list must not dismiss it.
    const onScroll = (e: Event) => {
      if (ref.current?.contains(e.target as Node)) return;
      onClose();
    };
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    window.addEventListener('scroll', onScroll, true);
    window.addEventListener('resize', onClose);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
      window.removeEventListener('scroll', onScroll, true);
      window.removeEventListener('resize', onClose);
    };
  }, [onClose, triggerRef]);

  return createPortal(
    <div
      ref={ref}
      role="menu"
      style={{
        position: 'fixed',
        left: pos?.x ?? -9999,
        top: pos?.y ?? -9999,
        visibility: pos ? 'visible' : 'hidden',
      }}
      className={`z-[100] rounded-card border border-discord-border bg-discord-panel p-1 shadow-discord-modal ${className}`}
    >
      {children}
    </div>,
    document.body
  );
}

/** Viewport coordinates for a menu hanging below a trigger button, right-aligned to it. */
export function menuAnchorFor(el: HTMLElement | null): { x: number; y: number } {
  if (!el) return { x: 0, y: 0 };
  const r = el.getBoundingClientRect();
  return { x: r.right, y: r.bottom + 4 };
}

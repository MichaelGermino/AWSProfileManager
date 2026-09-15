import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useLocation, useNavigate } from 'react-router';
import type { Tour, TourStep } from './tourSteps';

/**
 * Spotlight walkthrough of new features, shown once after an update.
 *
 * Three things make this harder than a stack of modals, and each shapes the code below:
 *
 * 1. **The screens are all mounted, but only the active one is visible.** `PersistentMainContent`
 *    hides the others with `display:none`, so an element on another route has no box to point at.
 *    A step therefore navigates first and then *polls* for its target — the element does not exist
 *    at the moment the step begins.
 * 2. **Some targets only exist once something is open.** The bulk-import entry lives in a menu, so
 *    the step opens it (`before`) and closes it again on the way out (`after`), in either
 *    direction, because the user can walk backwards.
 * 3. **Some targets do not exist at all for some users.** The folder button is only rendered when
 *    there is at least one profile. Those steps are marked `skipIfMissing` and drop out silently
 *    rather than showing a card pointing at nothing.
 *
 * The dimmer is one SVG rect masked by a rounded hole, not four divs around the target: a single
 * mask gives rounded corners and animates as one shape when the spotlight moves between steps.
 * It sits above everything except its own card and swallows every click — the tour drives itself,
 * and letting a stray click land on a half-highlighted control is how you get a tour that leaves
 * the app in a state the next step did not expect.
 */

interface Spot {
  top: number;
  left: number;
  width: number;
  height: number;
}

/** Breathing room between the target's own box and the edge of the hole. */
const SPOT_PADDING = 8;
const SPOT_RADIUS = 12;
const CARD_WIDTH = 380;
/** Gap between the hole and the card. */
const CARD_GAP = 14;
const VIEWPORT_MARGIN = 16;
/**
 * How long to wait for a step's target before giving up. Generous enough for a route change plus a
 * tab switch plus a scroll, short enough that a missing anchor does not read as a hang.
 */
const TARGET_TIMEOUT_MS = 2500;

/** All targets, or null if any one is missing or has no box (a hidden route, a closed menu). */
function resolveTargets(selectors: string[]): HTMLElement[] | null {
  const found: HTMLElement[] = [];
  for (const selector of selectors) {
    const el = document.querySelector<HTMLElement>(selector);
    if (!el || el.getClientRects().length === 0) return null;
    found.push(el);
  }
  return found;
}

/**
 * One box around every target. Needed because a trigger and the menu it opens are separate
 * elements — and the menu is absolutely positioned, so it is not inside the trigger's own rect.
 */
function unionSpot(els: HTMLElement[]): Spot {
  let top = Infinity;
  let left = Infinity;
  let right = -Infinity;
  let bottom = -Infinity;
  for (const el of els) {
    const r = el.getBoundingClientRect();
    top = Math.min(top, r.top);
    left = Math.min(left, r.left);
    right = Math.max(right, r.right);
    bottom = Math.max(bottom, r.bottom);
  }
  return {
    top: Math.round(top - SPOT_PADDING),
    left: Math.round(left - SPOT_PADDING),
    width: Math.round(right - left + SPOT_PADDING * 2),
    height: Math.round(bottom - top + SPOT_PADDING * 2),
  };
}

function sameSpot(a: Spot | null, b: Spot): boolean {
  return a !== null && a.top === b.top && a.left === b.left && a.width === b.width && a.height === b.height;
}

type Side = 'top' | 'bottom' | 'left' | 'right';
const SIDE_FALLBACK: Side[] = ['bottom', 'top', 'right', 'left'];

/** Card position: the preferred side if it fits on screen, else the first fallback that does. */
function placeCard(spot: Spot | null, cardHeight: number, preferred: TourStep['placement']) {
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const clampLeft = (l: number) => Math.min(Math.max(l, VIEWPORT_MARGIN), Math.max(VIEWPORT_MARGIN, vw - CARD_WIDTH - VIEWPORT_MARGIN));
  const clampTop = (t: number) => Math.min(Math.max(t, VIEWPORT_MARGIN), Math.max(VIEWPORT_MARGIN, vh - cardHeight - VIEWPORT_MARGIN));
  const centered = { top: clampTop((vh - cardHeight) / 2), left: clampLeft((vw - CARD_WIDTH) / 2) };
  if (!spot) return centered;

  const alignX = clampLeft(spot.left + spot.width / 2 - CARD_WIDTH / 2);
  const alignY = clampTop(spot.top + spot.height / 2 - cardHeight / 2);
  const candidates: Record<Side, { top: number; left: number; fits: boolean }> = {
    bottom: {
      top: spot.top + spot.height + CARD_GAP,
      left: alignX,
      fits: spot.top + spot.height + CARD_GAP + cardHeight <= vh - VIEWPORT_MARGIN,
    },
    top: {
      top: spot.top - CARD_GAP - cardHeight,
      left: alignX,
      fits: spot.top - CARD_GAP - cardHeight >= VIEWPORT_MARGIN,
    },
    right: {
      top: alignY,
      left: spot.left + spot.width + CARD_GAP,
      fits: spot.left + spot.width + CARD_GAP + CARD_WIDTH <= vw - VIEWPORT_MARGIN,
    },
    left: {
      top: alignY,
      left: spot.left - CARD_GAP - CARD_WIDTH,
      fits: spot.left - CARD_GAP - CARD_WIDTH >= VIEWPORT_MARGIN,
    },
  };

  const order: Side[] =
    preferred && preferred !== 'auto' ? [preferred, ...SIDE_FALLBACK] : SIDE_FALLBACK;
  for (const side of order) {
    const candidate = candidates[side];
    if (candidate.fits) return { top: Math.round(candidate.top), left: Math.round(candidate.left) };
  }

  /**
   * A target too large to sit beside — a whole settings section, say. Overlapping it is
   * unavoidable, so overlap the bottom rather than the middle: headings and the first controls
   * live at the top, and those are what the copy is talking about.
   */
  return { top: Math.round(clampTop(vh * 0.62)), left: Math.round(clampLeft((vw - CARD_WIDTH) / 2)) };
}

export function FeatureTour({
  tour,
  onFinish,
  debug = false,
}: {
  tour: Tour;
  /** `completed` distinguishes reaching the end from skipping; both mark the tour seen. */
  onFinish: (completed: boolean) => void;
  /**
   * Author mode, on under the dev flag. A missing anchor stops being invisible: instead of
   * degrading to a centered card or skipping, the step says which selector it could not find. A
   * renamed `data-tour` is otherwise silent, and silence is exactly how a tour rots between
   * releases.
   */
  debug?: boolean;
}) {
  const navigate = useNavigate();
  const location = useLocation();

  const [index, setIndex] = useState(0);
  const [spot, setSpot] = useState<Spot | null>(null);
  const [cardHeight, setCardHeight] = useState(0);
  const cardRef = useRef<HTMLDivElement | null>(null);
  /** Selectors this step could not resolve. Only surfaced in `debug`. */
  const [missingTargets, setMissingTargets] = useState<string[] | null>(null);
  /** Indices that dropped out because their anchor was missing; excluded from the step counter. */
  const [skipped, setSkipped] = useState<ReadonlySet<number>>(() => new Set());
  /**
   * Which way the user is walking. A skipped step must skip *onward* in the same direction —
   * otherwise Back off step 4 lands on the missing step 3, which skips forward to 4 again, and the
   * user can never get past it.
   */
  const directionRef = useRef<1 | -1>(1);

  // Refs, not deps: the step effect must not restart because the parent re-rendered or the route
  // settled. Restarting it would re-run `before` and re-scroll on every keystroke elsewhere.
  const pathRef = useRef(location.pathname);
  pathRef.current = location.pathname;
  const onFinishRef = useRef(onFinish);
  onFinishRef.current = onFinish;

  /**
   * Where the user was when the tour started. The tour moves them across routes to point at
   * things; ending on whichever screen the last step happened to need is disorienting, so it puts
   * them back where it found them.
   */
  const startPathRef = useRef(location.pathname);
  const finishRef = useRef<(completed: boolean) => void>(() => {});
  finishRef.current = (completed: boolean) => {
    if (pathRef.current !== startPathRef.current) navigate(startPathRef.current);
    onFinishRef.current(completed);
  };

  const gotoRef = useRef<(next: number, direction?: 1 | -1) => void>(() => {});
  gotoRef.current = (next: number, direction?: 1 | -1) => {
    if (direction) directionRef.current = direction;
    if (next < 0) return;
    // Walking off the end — including a final step that auto-skipped — is a completed tour.
    if (next >= tour.steps.length) {
      finishRef.current(true);
      return;
    }
    setIndex(next);
  };

  const step: TourStep | undefined = tour.steps[index];

  // Prepare the step, then track its target for as long as the step is on screen. Tracking rather
  // than measuring once is what keeps the hole on the control through the route transition, the
  // smooth scroll, and the menu's own open animation.
  useEffect(() => {
    const current = tour.steps[index];
    if (!current) return;
    let cancelled = false;
    setSpot(null);
    setMissingTargets(null);

    if (current.route && current.route !== pathRef.current) navigate(current.route);
    current.before?.();

    if (!current.targets?.length) {
      return () => current.after?.();
    }

    const targets = current.targets;
    const deadline = performance.now() + TARGET_TIMEOUT_MS;
    let raf = 0;
    let scrolled = false;
    let everResolved = false;

    const tick = () => {
      if (cancelled) return;
      const els = resolveTargets(targets);
      if (els) {
        if (!scrolled) {
          scrolled = true;
          els[0].scrollIntoView({ block: 'center', inline: 'nearest', behavior: 'smooth' });
        }
        everResolved = true;
        const next = unionSpot(els);
        setSpot((prev) => (sameSpot(prev, next) ? prev : next));
      } else if (!everResolved && performance.now() > deadline) {
        // Settled: the anchor is not coming.
        if (debug) {
          // Author mode: never skip, always say what is missing. A step that quietly disappears is
          // how a renamed data-tour ships unnoticed.
          console.warn(`[tour] step "${current.id}": no element matched ${targets.join(', ')}`);
          setMissingTargets(targets);
          return;
        }
        if (current.skipIfMissing) {
          setSkipped((prev) => (prev.has(index) ? prev : new Set(prev).add(index)));
          gotoRef.current(index + directionRef.current);
        }
        // Otherwise stop polling and leave `spot` null — the card still reads fine centered, which
        // beats vanishing a step because one selector drifted.
        return;
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);

    return () => {
      cancelled = true;
      cancelAnimationFrame(raf);
      current.after?.();
    };
  }, [index, tour, navigate, debug]);

  // The card's height decides which side it can sit on, and it changes with the copy.
  useEffect(() => {
    const el = cardRef.current;
    if (!el) return;
    const update = () => setCardHeight(el.offsetHeight);
    update();
    const observer = new ResizeObserver(update);
    observer.observe(el);
    return () => observer.disconnect();
  }, [index]);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        // Capture phase + stopPropagation so Escape ends the tour instead of being eaten by the
        // dismiss handler of whatever the current step opened.
        e.stopPropagation();
        finishRef.current(false);
      } else if (e.key === 'ArrowRight') {
        gotoRef.current(index + 1, 1);
      } else if (e.key === 'ArrowLeft') {
        gotoRef.current(index - 1, -1);
      }
    };
    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, [index]);

  if (!step) return null;

  const position = placeCard(spot, cardHeight, step.placement);
  const isLast = index === tour.steps.length - 1;
  // Counted over the steps this user actually sees, so a skipped anchor does not show as "2 of 5"
  // followed by "4 of 5".
  const stepTotal = tour.steps.length - skipped.size;
  const stepNumber = tour.steps.slice(0, index + 1).filter((_, i) => !skipped.has(i)).length;

  return createPortal(
    <div data-tour-overlay>
      {/* Blocks every click underneath. The tour navigates itself; a stray click on a
          half-highlighted control would desync the next step. */}
      <div
        className="fixed inset-0 z-[120]"
        onMouseDown={(e) => e.preventDefault()}
        aria-hidden
      >
        <svg className="h-full w-full" width="100%" height="100%">
          <defs>
            <mask id="feature-tour-spotlight">
              <rect x="0" y="0" width="100%" height="100%" fill="white" />
              {spot && (
                <rect
                  x={spot.left}
                  y={spot.top}
                  width={spot.width}
                  height={spot.height}
                  rx={SPOT_RADIUS}
                  fill="black"
                  style={{ transition: 'x 200ms ease, y 200ms ease, width 200ms ease, height 200ms ease' }}
                />
              )}
            </mask>
          </defs>
          <rect x="0" y="0" width="100%" height="100%" fill="rgba(0,0,0,0.66)" mask="url(#feature-tour-spotlight)" />
        </svg>
        {spot && (
          <div
            className="pointer-events-none absolute rounded-[12px] ring-2 ring-discord-accent transition-all duration-200"
            style={{ top: spot.top, left: spot.left, width: spot.width, height: spot.height }}
          />
        )}
      </div>

      <div
        ref={cardRef}
        role="dialog"
        aria-modal="true"
        aria-label={`${step.title} — step ${stepNumber} of ${stepTotal}`}
        className="fixed z-[130] rounded-card border border-discord-border bg-discord-panel shadow-discord-modal"
        // Inline, not a Tailwind class: `transition-[top,left]` is silently dropped by the
        // arbitrary-value parser (the comma), leaving the card to jump between steps.
        style={{
          top: position.top,
          left: position.left,
          width: CARD_WIDTH,
          transition: 'top 200ms ease, left 200ms ease',
        }}
      >
        <div className="px-5 pt-5">
          <div className="flex items-start justify-between gap-3">
            <h2 className="text-base font-semibold text-discord-text">{step.title}</h2>
            <span className="mt-0.5 shrink-0 text-xs text-discord-textMuted">
              {stepNumber} / {stepTotal}
            </span>
          </div>
          <p className="mt-2 text-sm leading-relaxed text-discord-textMuted">{step.body}</p>
          {debug && missingTargets && (
            <p className="mt-3 rounded-panel border border-discord-warning/40 bg-discord-warning/10 px-3 py-2 font-mono text-xs leading-relaxed text-discord-warning">
              anchor not found: {missingTargets.join(', ')}
            </p>
          )}
        </div>

        <div className="mt-4 flex items-center justify-between gap-3 border-t border-discord-border px-5 py-3">
          <button
            type="button"
            onClick={() => finishRef.current(false)}
            className="text-xs text-discord-textMuted transition-colors hover:text-discord-text"
          >
            Skip tour
          </button>
          <div className="flex items-center gap-2">
            {index > 0 && (
              <button
                type="button"
                onClick={() => gotoRef.current(index - 1, -1)}
                className="rounded-button border border-discord-border bg-discord-darkest px-3 py-1.5 text-sm text-discord-textMuted transition-colors hover:bg-discord-dark hover:text-discord-text"
              >
                Back
              </button>
            )}
            <button
              type="button"
              autoFocus
              onClick={() => gotoRef.current(index + 1, 1)}
              className="rounded-button bg-discord-accent px-4 py-1.5 text-sm font-medium text-white transition-colors hover:bg-discord-accentHover"
            >
              {isLast ? 'Done' : 'Next'}
            </button>
          </div>
        </div>
      </div>
    </div>,
    document.body
  );
}

export default FeatureTour;

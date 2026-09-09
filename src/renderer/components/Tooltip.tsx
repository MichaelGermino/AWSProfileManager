import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';

const TOOLTIP_DELAY_MS = 1000;

type TooltipPlacement = 'above' | 'below' | 'left' | 'right';
type TooltipAlign = 'left' | 'center' | 'right';
type WrapWidth = 'sm' | 'md' | 'lg' | 'xl' | '2xl';

export function Tooltip({
  label,
  placement = 'below',
  align = 'center',
  wrap = false,
  wrapWidth = 'sm',
  usePortal = false,
  children,
}: {
  label: string;
  placement?: TooltipPlacement;
  align?: TooltipAlign;
  /** When true, tooltip text wraps with a max width instead of staying on one line. */
  wrap?: boolean;
  /** When wrap is true, use a wider max-width so the tooltip is less tall. Default 'sm'. */
  wrapWidth?: WrapWidth;
  /** When true, render tooltip in a portal so it is not clipped by an overflow container. Works with every placement. */
  usePortal?: boolean;
  children: React.ReactNode;
}) {
  const [visible, setVisible] = useState(false);
  const [portalRect, setPortalRect] = useState<DOMRect | null>(null);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const triggerRef = useRef<HTMLDivElement>(null);
  const isAbove = placement === 'above';
  const isBelow = placement === 'below';
  const isRight = placement === 'right' || align === 'right';
  const isLeft = placement === 'left' || align === 'left';
  const isPlacementRight = placement === 'right';
  const isPlacementLeft = placement === 'left';

  const handleEnter = () => {
    timeoutRef.current = setTimeout(() => setVisible(true), TOOLTIP_DELAY_MS);
  };

  const handleLeave = () => {
    if (timeoutRef.current) {
      clearTimeout(timeoutRef.current);
      timeoutRef.current = null;
    }
    setVisible(false);
    if (usePortal) setPortalRect(null);
  };

  useEffect(() => () => {
    if (timeoutRef.current) clearTimeout(timeoutRef.current);
  }, []);

  useLayoutEffect(() => {
    if (visible && usePortal && triggerRef.current) {
      setPortalRect(triggerRef.current.getBoundingClientRect());
    } else if (!visible && usePortal) {
      setPortalRect(null);
    }
  }, [visible, usePortal]);

  const wrapClasses = wrap
    ? `whitespace-normal min-w-[40ch] ${
        wrapWidth === '2xl'
          ? 'max-w-xl'
          : wrapWidth === 'xl'
            ? 'max-w-lg'
            : wrapWidth === 'lg'
              ? 'max-w-lg'
              : wrapWidth === 'md'
                ? 'max-w-md'
                : 'max-w-sm'
      }`
    : 'whitespace-nowrap';

  const baseClasses = `px-3 py-2 rounded-button bg-discord-panel text-discord-text text-xs font-medium shadow-discord-modal border border-discord-border z-[100] ${wrapClasses}`;

  const renderInline = visible && !usePortal && (
    <div
      className={`absolute ${baseClasses} ${
        isPlacementRight
          ? 'left-full ml-1.5 top-0'
          : isPlacementLeft
            ? 'right-full mr-1.5 top-0'
            : isRight
              ? 'right-0'
              : isLeft
                ? 'left-0'
                : 'left-1/2 -translate-x-1/2'
      } ${isAbove ? 'bottom-full mb-1.5' : ''} ${isBelow && !isPlacementRight && !isPlacementLeft ? 'top-full mt-1.5' : ''}`}
      role="tooltip"
    >
      {isPlacementRight ? (
        <span className="absolute right-full top-1/2 -translate-y-1/2 w-0 h-0 border-[5px] border-transparent border-r-discord-panel" aria-hidden />
      ) : isPlacementLeft ? (
        <span className="absolute left-full top-1/2 -translate-y-1/2 w-0 h-0 border-[5px] border-transparent border-l-discord-panel" aria-hidden />
      ) : isAbove && !isPlacementRight && !isPlacementLeft ? (
        <span
          className={`absolute top-full w-0 h-0 border-[5px] border-transparent border-t-discord-panel ${isRight ? 'right-3 -translate-x-0' : isLeft ? 'left-3' : 'left-1/2 -translate-x-1/2'}`}
          aria-hidden
        />
      ) : isBelow && !isPlacementRight && !isPlacementLeft ? (
        <span
          className={`absolute bottom-full w-0 h-0 border-[5px] border-transparent border-b-discord-panel ${isRight ? 'right-3 -translate-x-0' : isLeft ? 'left-3' : 'left-1/2 -translate-x-1/2'}`}
          aria-hidden
        />
      ) : null}
      {label}
    </div>
  );

  const GAP = 6;
  const portalContent =
    visible && usePortal && portalRect && isPlacementRight
      ? createPortal(
          <div
            className={`fixed ${baseClasses}`}
            style={{
              left: portalRect.right + GAP,
              top: portalRect.top,
            }}
            role="tooltip"
          >
            <span className="absolute right-full top-1/2 -translate-y-1/2 w-0 h-0 border-[5px] border-transparent border-r-discord-panel" aria-hidden />
            {label}
          </div>,
          document.body
        )
      : null;

  /**
   * Portal path for every placement other than 'right'.
   *
   * Without this, `usePortal` combined with placement 'above'/'below'/'left' rendered NOTHING: the
   * portal branch above requires isPlacementRight, and renderInline requires !usePortal, so both
   * bailed and the tooltip silently never appeared.
   *
   * Positioned with transforms rather than measured height, so it needs no second layout pass.
   */
  const portalFallback = (() => {
    if (!(visible && usePortal && portalRect && !isPlacementRight)) return null;

    let left: number;
    let top: number;
    let tx = '0';
    let ty = '0';

    if (isPlacementLeft) {
      left = portalRect.left - GAP;
      top = portalRect.top;
      tx = '-100%';
    } else {
      top = isAbove ? portalRect.top - GAP : portalRect.bottom + GAP;
      if (isAbove) ty = '-100%';
      if (align === 'right') {
        left = portalRect.right;
        tx = '-100%';
      } else if (align === 'left') {
        left = portalRect.left;
      } else {
        left = portalRect.left + portalRect.width / 2;
        tx = '-50%';
      }
    }

    const arrowSide = isAbove ? 'top-full border-t-discord-panel' : 'bottom-full border-b-discord-panel';
    const arrowX = align === 'right' ? 'right-3' : align === 'left' ? 'left-3' : 'left-1/2 -translate-x-1/2';

    return createPortal(
      <div
        className={`fixed ${baseClasses}`}
        style={{ left, top, transform: `translate(${tx}, ${ty})` }}
        role="tooltip"
      >
        {!isPlacementLeft && (
          <span
            className={`absolute w-0 h-0 border-[5px] border-transparent ${arrowSide} ${arrowX}`}
            aria-hidden
          />
        )}
        {label}
      </div>,
      document.body
    );
  })();

  return (
    <div
      ref={triggerRef}
      className="relative inline-flex"
      onMouseEnter={handleEnter}
      onMouseLeave={handleLeave}
    >
      {children}
      {renderInline}
      {portalContent}
      {portalFallback}
    </div>
  );
}

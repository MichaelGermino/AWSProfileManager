import { useEffect, useRef, useState } from 'react';

const TOOLTIP_DELAY_MS = 1000;

type TooltipPlacement = 'above' | 'below' | 'left' | 'right';
type TooltipAlign = 'left' | 'center' | 'right';

export function Tooltip({
  label,
  placement = 'below',
  align = 'center',
  children,
}: {
  label: string;
  placement?: TooltipPlacement;
  align?: TooltipAlign;
  children: React.ReactNode;
}) {
  const [visible, setVisible] = useState(false);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
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
  };

  useEffect(() => () => {
    if (timeoutRef.current) clearTimeout(timeoutRef.current);
  }, []);

  return (
    <div
      className="relative inline-flex"
      onMouseEnter={handleEnter}
      onMouseLeave={handleLeave}
    >
      {children}
      {visible && (
        <div
          className={`absolute px-3 py-2 rounded-button bg-discord-panel text-discord-text text-xs font-medium whitespace-nowrap shadow-discord-modal border border-discord-border z-[100] ${
            isPlacementRight
              ? 'left-full ml-1.5 top-1/2 -translate-y-1/2'
              : isPlacementLeft
                ? 'right-full mr-1.5 top-1/2 -translate-y-1/2'
                : isRight
                  ? 'right-0'
                  : isLeft
                    ? 'left-0'
                    : 'left-1/2 -translate-x-1/2'
          } ${isAbove ? 'bottom-full mb-1.5' : ''} ${isBelow && !isPlacementRight && !isPlacementLeft ? 'top-full mt-1.5' : ''}`}
          role="tooltip"
        >
          {isAbove && !isPlacementRight && !isPlacementLeft ? (
            <span
              className={`absolute top-full w-0 h-0 border-[5px] border-transparent border-t-discord-panel ${isRight ? 'right-3 -translate-x-0' : isLeft ? 'left-3' : 'left-1/2 -translate-x-1/2'}`}
              aria-hidden
            />
          ) : isBelow && !isPlacementRight && !isPlacementLeft ? (
            <span
              className={`absolute bottom-full w-0 h-0 border-[5px] border-transparent border-b-discord-panel ${isRight ? 'right-3 -translate-x-0' : isLeft ? 'left-3' : 'left-1/2 -translate-x-1/2'}`}
              aria-hidden
            />
          ) : isPlacementRight ? (
            <span className="absolute right-full top-1/2 -translate-y-1/2 w-0 h-0 border-[5px] border-transparent border-r-discord-panel" aria-hidden />
          ) : isPlacementLeft ? (
            <span className="absolute left-full top-1/2 -translate-y-1/2 w-0 h-0 border-[5px] border-transparent border-l-discord-panel" aria-hidden />
          ) : null}
          {label}
        </div>
      )}
    </div>
  );
}

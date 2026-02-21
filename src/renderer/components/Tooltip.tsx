import { useEffect, useRef, useState } from 'react';

const TOOLTIP_DELAY_MS = 1000;

type TooltipPlacement = 'above' | 'below';
type TooltipAlign = 'center' | 'right';

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
  const isRight = align === 'right';

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
          className={`absolute px-2.5 py-1.5 rounded-md bg-discord-darkest text-discord-text text-xs font-medium whitespace-nowrap shadow-lg border border-discord-panel z-[100] ${
            isRight ? 'right-0' : 'left-1/2 -translate-x-1/2'
          } ${isAbove ? 'bottom-full mb-1.5' : 'top-full mt-1.5'}`}
          role="tooltip"
        >
          {isAbove ? (
            <span
              className={`absolute top-full w-0 h-0 border-[5px] border-transparent border-t-discord-darkest ${isRight ? 'right-3 -translate-x-0' : 'left-1/2 -translate-x-1/2'}`}
              aria-hidden
            />
          ) : (
            <span
              className={`absolute bottom-full w-0 h-0 border-[5px] border-transparent border-b-discord-darkest ${isRight ? 'right-3 -translate-x-0' : 'left-1/2 -translate-x-1/2'}`}
              aria-hidden
            />
          )}
          {label}
        </div>
      )}
    </div>
  );
}

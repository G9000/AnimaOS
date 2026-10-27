import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
} from 'react';

function minimumShare(width: number) {
  return width > 0 ? Math.min(50, Math.max(20, (320 / width) * 100)) : 20;
}

function clamp(value: number, minimum: number) {
  return Math.min(100 - minimum, Math.max(minimum, value));
}

export function useArtifactResize() {
  const layoutRef = useRef<HTMLDivElement>(null);
  const pointer = useRef<number | null>(null);
  const [preferredShare, setPreferredShare] = useState(50);
  const [layoutWidth, setLayoutWidth] = useState(0);
  const [resizing, setResizing] = useState(false);
  const minimum = minimumShare(layoutWidth);
  const chatShare = clamp(preferredShare, minimum);
  const stopResizing = useCallback(() => {
    pointer.current = null;
    setResizing(false);
  }, []);

  useEffect(() => {
    const layout = layoutRef.current;
    if (!layout) return;
    const measure = () => setLayoutWidth(layout.getBoundingClientRect().width);
    measure();
    const observer =
      typeof ResizeObserver === 'undefined'
        ? null
        : new ResizeObserver(measure);
    observer?.observe(layout);
    window.addEventListener('resize', measure);
    window.addEventListener('blur', stopResizing);
    return () => {
      observer?.disconnect();
      window.removeEventListener('resize', measure);
      window.removeEventListener('blur', stopResizing);
    };
  }, [stopResizing]);

  const finish = (event: PointerEvent<HTMLDivElement>) => {
    if (pointer.current === event.pointerId) stopResizing();
  };

  return {
    layoutRef,
    chatShare,
    minimum,
    resizing,
    stopResizing,
    dividerHandlers: {
      onPointerDown(event: PointerEvent<HTMLDivElement>) {
        if (event.button !== 0 || pointer.current !== null) return;
        event.preventDefault();
        event.currentTarget.focus();
        event.currentTarget.setPointerCapture(event.pointerId);
        pointer.current = event.pointerId;
        setResizing(true);
      },
      onPointerMove(event: PointerEvent<HTMLDivElement>) {
        if (pointer.current !== event.pointerId) return;
        const rect = layoutRef.current?.getBoundingClientRect();
        if (!rect || rect.width <= 0) return;
        setLayoutWidth(rect.width);
        setPreferredShare(
          clamp(
            ((event.clientX - rect.left) / rect.width) * 100,
            minimumShare(rect.width),
          ),
        );
      },
      onPointerUp: finish,
      onPointerCancel: finish,
      onLostPointerCapture: finish,
      onDoubleClick: () => setPreferredShare(50),
      onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
        const step = event.shiftKey ? 10 : 2;
        const next =
          event.key === 'ArrowLeft'
            ? chatShare - step
            : event.key === 'ArrowRight'
              ? chatShare + step
              : event.key === 'Home'
                ? minimum
                : event.key === 'End'
                  ? 100 - minimum
                  : undefined;
        if (next === undefined) return;
        event.preventDefault();
        setPreferredShare(clamp(next, minimum));
      },
    },
  };
}

import { useLayoutEffect, useRef } from "react";
import type { ReactNode, RefObject, UIEventHandler } from "react";
import type { Page } from "../app-types";

export function WorkspaceFrame({ page, toolbar, scrollRef, onScroll, children }: {
  page: Page;
  toolbar?: ReactNode;
  scrollRef: RefObject<HTMLDivElement | null>;
  onScroll: UIEventHandler<HTMLDivElement>;
  children: ReactNode;
}) {
  const frameRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const frame = frameRef.current;
    if (page !== "live" || !frame) return;
    // Use available layout pixels so panel limits also follow interface scaling.
    const resize = () => frame.style.setProperty("--live-workspace-height", `${frame.clientHeight}px`);
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(frame);
    return () => observer.disconnect();
  }, [page]);

  return <div className="workspace-frame" ref={frameRef}>
    {page === "live" && toolbar && <div className="live-toolbar">{toolbar}</div>}
    <div className="app-scroll-region" ref={scrollRef} onScroll={onScroll}>
      <main className={`workspace workspace-${page}`}>{children}</main>
    </div>
  </div>;
}

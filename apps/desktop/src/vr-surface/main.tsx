import { Component, StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { connectSurfaceBridge, type SurfaceBridge } from "./bridge";
import { SurfaceHost } from "./SurfaceHost";
import { isSurface } from "./protocol";
import "../styles.css";

let connection: SurfaceBridge | null = null;
function fault(stage: "startup" | "render" | "unhandled") {
  void connection?.fault(stage).catch(() => {});
}
const connect: typeof connectSurfaceBridge = async (surface, signal) => {
  const bridge = await connectSurfaceBridge(surface, signal);
  connection = bridge;
  signal.addEventListener("abort", () => { if (connection === bridge) connection = null; }, { once: true });
  return bridge;
};
class Boundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() { return { failed: true }; }
  componentDidCatch() { fault("render"); }
  render() { return this.state.failed ? null : this.props.children; }
}
window.addEventListener("error", () => fault("unhandled"));
window.addEventListener("unhandledrejection", () => fault("unhandled"));
const surface = new URLSearchParams(window.location.search).get("surface");
if (isSurface(surface)) {
  document.documentElement.dataset.vrSurface = surface;
  createRoot(document.getElementById("root")!).render(<StrictMode><Boundary><SurfaceHost surface={surface} connect={connect} /></Boundary></StrictMode>);
} else {
  fault("startup");
}

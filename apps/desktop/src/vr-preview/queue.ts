export type PreviewOperation = { type: string; [key: string]: unknown };

// Coalesce motion only between button transitions. Down, Up and commands remain ordered.
export function previewQueue(run: (operation: PreviewOperation) => Promise<void>) {
  const pending: PreviewOperation[] = [];
  let running = false;
  let closed = false;
  async function drain() {
    if (running || closed) return;
    running = true;
    try {
      while (pending.length && !closed) await run(pending.shift()!);
    } finally { running = false; }
  }
  return {
    push(operation: PreviewOperation) {
      if (closed) return;
      const last = pending.at(-1);
      if (operation.type === "refresh" && (running || pending.length)) return;
      if (last?.type === "pointer" && operation.type === "pointer") {
        const a = last.input as Record<string, unknown>;
        const b = operation.input as Record<string, unknown>;
        if (["buttons", "panel", "generation", "revision"].every((key) => a[key] === b[key])) pending.pop();
      }
      if (operation.type === "cancel") {
        for (let i = pending.length - 1; i >= 0; i--) if (pending[i].type === "pointer") pending.splice(i, 1);
      }
      pending.push(operation);
      void drain();
    },
    close() { closed = true; pending.length = 0; },
  };
}

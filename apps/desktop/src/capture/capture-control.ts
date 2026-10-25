export function createCaptureController(options: {
  available: () => boolean;
  running: () => boolean;
  newConversation: () => boolean;
  createConversation: () => Promise<boolean>;
  beforeStart?: () => Promise<boolean>;
  setRunning: (target: boolean) => Promise<boolean>;
  onPending: (pending: boolean) => void;
}) {
  let pending: { target: boolean; result: Promise<boolean> } | null = null;
  const setRunning = (target: boolean): Promise<boolean> => {
    if (pending) return pending.target === target ? pending.result : Promise.resolve(false);
    if (!options.available()) return Promise.resolve(false);
    if (options.running() === target) return Promise.resolve(true);
    options.onPending(true);
    const result = (async () => {
      if (target && options.beforeStart && !await options.beforeStart()) return false;
      if (target && options.newConversation() && !await options.createConversation()) return false;
      return options.setRunning(target);
    })().finally(() => {
      pending = null;
      options.onPending(false);
    });
    pending = { target, result };
    return result;
  };
  return { setRunning, toggle: () => setRunning(!options.running()) };
}

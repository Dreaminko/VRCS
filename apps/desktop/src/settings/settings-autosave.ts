type SettingsAutosaveOptions<T> = {
  persist: (value: T) => Promise<T>;
  getCurrent?: () => T | null;
  reload?: () => Promise<T>;
  onOptimistic: (value: T) => void;
  onCommit: (value: T) => void;
  onError: (reason: unknown) => void;
};

export function createSettingsAutosave<T>({
  persist,
  getCurrent,
  reload,
  onOptimistic,
  onCommit,
  onError,
}: SettingsAutosaveOptions<T>) {
  type Waiter = {
    resolve: (saved: T) => void;
    reject: (reason: unknown) => void;
  };
  type PendingSave = {
    value: T;
    version: number;
    waiters: Waiter[];
    update?: (current: T) => T;
  };

  let latestVersion = 0;
  let saving = false;
  const pending: PendingSave[] = [];
  let desired: T | null = null;

  const persistUpdate = async (update: (current: T) => T): Promise<T> => {
    let current = getCurrent?.();
    if (current == null) throw new Error("Core is unavailable");
    for (let attempt = 0; ; attempt++) {
      const next = update(current);
      if (next === current) return current;
      try {
        return await persist(next);
      } catch (reason) {
        if (attempt > 0 || !reload || (reason as { status?: number })?.status !== 409) throw reason;
        current = await reload();
      }
    }
  };

  const flush = async (save: PendingSave): Promise<void> => {
    saving = true;
    try {
      const saved = await (save.update ? persistUpdate(save.update) : persist(save.value));
      if (save.version === latestVersion) onCommit(saved);
      save.waiters.forEach(({ resolve }) => resolve(saved));
    } catch (reason) {
      if (save.version === latestVersion) onError(reason);
      save.waiters.forEach(({ reject }) => reject(reason));
    } finally {
      const next = pending.shift();
      if (next) void flush(next);
      else { saving = false; desired = null; }
    }
  };

  const enqueue = (value: T, update?: (current: T) => T): Promise<T> => {
    const version = ++latestVersion;
    desired = value;
    onOptimistic(value);

    return new Promise<T>((resolve, reject) => {
      if (saving) {
        const last = pending.at(-1);
        if (last && !last.update && !update) {
          last.value = value;
          last.version = version;
          last.waiters.push({ resolve, reject });
        } else {
          pending.push({ value, update, version, waiters: [{ resolve, reject }] });
        }
        return;
      }
      void flush({ value, update, version, waiters: [{ resolve, reject }] });
    });
  };

  return Object.assign((value: T) => enqueue(value), {
    update: (update: (current: T) => T): Promise<T> => {
      try {
        const current = saving ? desired : getCurrent?.();
        if (current == null) throw new Error("Core is unavailable");
        // Queue patches separately so each control receives its own confirmed result.
        return enqueue(update(current), update);
      } catch (reason) {
        return Promise.reject(reason);
      }
    },
  });
}

type SettingsAutosaveOptions<T> = {
  persist: (value: T) => Promise<T>;
  getCurrent?: () => T | null;
  reload?: () => Promise<T>;
  onOptimistic: (value: T) => void;
  onCommit: (value: T) => void;
  onError: (reason: unknown) => void;
};

export interface SettingsUpdateOptions<T = unknown> {
  coalesce?: boolean;
  base?: T;
}

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
    coalesce?: boolean;
    base?: T;
  };

  let latestVersion = 0;
  let saving = false;
  const pending: PendingSave[] = [];
  let desired: T | null = null;
  let lastPersisted: T | null = null;

  const readCurrent = (fallbackBase?: T): T | null =>
    getCurrent?.() ?? lastPersisted ?? fallbackBase ?? null;

  const persistUpdate = async (
    update: (current: T) => T,
    fallbackBase?: T,
  ): Promise<T> => {
    let current = readCurrent(fallbackBase);
    if (current == null) throw new Error("Core is unavailable");
    for (let attempt = 0; ; attempt++) {
      const next = update(current);
      if (next === current) return current;
      try {
        return await persist(next);
      } catch (reason) {
        const conflict = reason as { status?: number; code?: string } | null;
        if (
          attempt > 0
          || !reload
          || conflict?.status !== 409
          || (conflict?.code !== undefined && conflict.code !== "settings.stale")
        ) {
          throw reason;
        }
        current = await reload();
      }
    }
  };

  const flush = async (save: PendingSave): Promise<void> => {
    saving = true;
    try {
      const saved = await (save.update
        ? persistUpdate(save.update, save.base)
        : persist(save.value));
      lastPersisted = saved;
      if (save.version === latestVersion) onCommit(saved);
      save.waiters.forEach(({ resolve }) => resolve(saved));
    } catch (reason) {
      if (save.update || save.version === latestVersion) onError(reason);
      save.waiters.forEach(({ reject }) => reject(reason));
    } finally {
      const next = pending.shift();
      if (next) void flush(next);
      else { saving = false; desired = null; }
    }
  };

  const enqueue = (
    value: T,
    update?: (current: T) => T,
    options?: SettingsUpdateOptions<T>,
  ): Promise<T> => {
    const version = ++latestVersion;
    const coalesce = Boolean(options?.coalesce);
    desired = value;
    onOptimistic(value);

    return new Promise<T>((resolve, reject) => {
      if (saving) {
        const last = pending.at(-1);
        if (last && !last.update && !update) {
          last.value = value;
          last.version = version;
          last.waiters.push({ resolve, reject });
        } else if (last && last.coalesce && coalesce && last.update && update) {
          const previousUpdate = last.update;
          last.update = (current) => update(previousUpdate(current));
          last.value = value;
          last.version = version;
          last.waiters.push({ resolve, reject });
        } else {
          pending.push({
            value,
            update,
            coalesce,
            base: options?.base,
            version,
            waiters: [{ resolve, reject }],
          });
        }
        return;
      }
      void flush({
        value,
        update,
        coalesce,
        base: options?.base,
        version,
        waiters: [{ resolve, reject }],
      });
    });
  };

  return Object.assign((value: T) => enqueue(value), {
    getDesired: (): T | null => desired ?? readCurrent(),
    update: (update: (current: T) => T, options?: SettingsUpdateOptions<T>): Promise<T> => {
      try {
        const current = saving ? desired : readCurrent(options?.base);
        if (current == null) throw new Error("Core is unavailable");
        // Queue patches separately unless the caller explicitly opts into coalescing.
        return enqueue(update(current), update, options);
      } catch (reason) {
        return Promise.reject(reason);
      }
    },
  });
}

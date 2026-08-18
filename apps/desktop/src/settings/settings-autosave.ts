type SettingsAutosaveOptions<T> = {
  persist: (value: T) => Promise<T>;
  onOptimistic: (value: T) => void;
  onCommit: (value: T) => void;
  onError: (reason: unknown) => void;
};

export function createSettingsAutosave<T>({
  persist,
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
  };

  let latestVersion = 0;
  let saving = false;
  let pending: PendingSave | null = null;

  const flush = async (save: PendingSave): Promise<void> => {
    saving = true;
    try {
      const saved = await persist(save.value);
      if (save.version === latestVersion) onCommit(saved);
      save.waiters.forEach(({ resolve }) => resolve(saved));
    } catch (reason) {
      if (save.version === latestVersion) onError(reason);
      save.waiters.forEach(({ reject }) => reject(reason));
    } finally {
      const next = pending;
      pending = null;
      if (next) void flush(next);
      else saving = false;
    }
  };

  return (value: T): Promise<T> => {
    const version = ++latestVersion;
    onOptimistic(value);

    return new Promise<T>((resolve, reject) => {
      if (saving) {
        if (pending) {
          pending.value = value;
          pending.version = version;
          pending.waiters.push({ resolve, reject });
        } else {
          pending = { value, version, waiters: [{ resolve, reject }] };
        }
        return;
      }
      void flush({ value, version, waiters: [{ resolve, reject }] });
    });
  };
}

export type SettingsAutosaveOptions<T> = {
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
  let latestVersion = 0;
  let queue: Promise<void> = Promise.resolve();

  return (value: T): Promise<T> => {
    const version = ++latestVersion;
    onOptimistic(value);

    const request = queue.then(() => persist(value));
    queue = request.then(
      () => undefined,
      () => undefined,
    );

    return request.then(
      (saved) => {
        if (version === latestVersion) onCommit(saved);
        return saved;
      },
      (reason) => {
        if (version === latestVersion) onError(reason);
        throw reason;
      },
    );
  };
}

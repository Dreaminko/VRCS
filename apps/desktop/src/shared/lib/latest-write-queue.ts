export class LatestWriteQueue<T> {
  private pending: T | null = null;
  private task: Promise<void> | null = null;
  private readonly write: (value: T) => Promise<void>;

  constructor(write: (value: T) => Promise<void>) {
    this.write = write;
  }

  enqueue(value: T): Promise<void> {
    this.pending = value;
    this.task ??= this.flush();
    return this.task;
  }

  private async flush(): Promise<void> {
    try {
      while (this.pending !== null) {
        const value = this.pending;
        this.pending = null;
        await this.write(value);
      }
    } finally {
      this.task = null;
      if (this.pending !== null) this.task = this.flush();
    }
  }
}

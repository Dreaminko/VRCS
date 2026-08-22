export interface DatabaseStorageStats {
  used_bytes: number;
  allocated_bytes: number;
  reclaimable_bytes: number;
  max_bytes: number;
  over_limit: boolean;
}

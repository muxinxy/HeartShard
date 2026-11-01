// HeartShard Tauri 命令的类型化封装（M1）。
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";

export interface SessionStatus {
  unlocked: boolean;
}

export type ImportStatus = "imported" | "skipped_duplicate" | "failed";

export interface ImportOutcome {
  path: string;
  status: ImportStatus;
  blob_hash: string | null;
  error: string | null;
}

export interface BlobEntry {
  blob_hash: string;
  file_name: string;
  mime: string;
  total_len: number;
  taken_at: number; // unix 秒
  imported_at: number;
}

export interface VerifyReport {
  blob_hash: string;
  ok: boolean;
  missing: string[];
  corrupted: string[];
  shard_count: number;
}

export interface SyncOutcome {
  published_lamport: number;
  pulled_objects: number;
  applied_objects: number;
  skipped: string[];
  stores_ok: string[];
  stores_failed: string[];
}

export interface StorageStatus {
  id: string;
  kind: string;
}

export interface RepairReport {
  blob_hash: string;
  repaired: string[];
  unrecoverable: number[];
  shard_count: number;
}

export const api = {
  sessionStatus: () => invoke<SessionStatus>("session_status"),
  unlock: (password: string) => invoke<SessionStatus>("unlock", { password }),
  lock: () => invoke<SessionStatus>("lock"),

  importFiles: (paths: string[]) => invoke<ImportOutcome[]>("import_files", { paths }),
  listBlobs: () => invoke<BlobEntry[]>("list_blobs"),
  exportBlob: (blobHash: string, destPath: string) =>
    invoke<void>("export_blob", { blobHash, destPath }),
  verifyBlob: (blobHash: string) => invoke<VerifyReport>("verify_blob", { blobHash }),
  syncNow: () => invoke<SyncOutcome>("sync_now"),
  repairBlob: (blobHash: string) => invoke<RepairReport>("repair_blob", { blobHash }),
  storageStatus: () => invoke<StorageStatus[]>("storage_status"),

  pickFiles: async (): Promise<string[]> => {
    const picked = await open({
      multiple: true,
      title: "选择要导入的照片 / 视频",
    });
    if (!picked) return [];
    return Array.isArray(picked) ? picked : [picked];
  },

  pickSavePath: async (defaultName: string): Promise<string | null> =>
    save({ title: "导出位置", defaultPath: defaultName }),
};

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export function formatDate(unixSecs: number): string {
  if (!unixSecs) return "—";
  return new Date(unixSecs * 1000).toLocaleString();
}

export function shortHash(hash: string): string {
  return hash.length > 16 ? `${hash.slice(0, 8)}…${hash.slice(-6)}` : hash;
}

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

export interface RebalanceReport {
  blobs_checked: number;
  moved: number;
  failed: string[];
}

export interface DirScanResult {
  session_id: string;
  files_scanned: number;
  total_bytes: number;
  dirs: string[];
  truncated: string[];
  symlinks: string[];
}

export interface PreviewAlbum {
  rel_path: string;
  file_count: number;
  bytes: number;
}

export interface ImportPreview {
  session_id: string;
  albums: PreviewAlbum[];
  total_files: number;
  total_bytes: number;
  already_in_index: number;
  truncated: string[];
  symlinks: string[];
}

export interface ImportRunReport {
  session_id: string;
  albums_created: number;
  albums_reused: number;
  imported: number;
  skipped_duplicate: number;
  failed: string[];
  paused: boolean;
  truncated: string[];
  symlinks: string[];
}

export interface SettingsData {
  chunk_size: number;
  n: number;
  m: number;
  download_concurrency: number;
  theme: string;
}

export interface AlbumInfo {
  id: string;
  name: string;
  parent_id: string | null;
  sort_order: number;
  cover_hash: string;
  blob_count: number;
}

export interface ImportProgress {
  session_id: string;
  stage: string;
  album_name: string | null;
  album_index: number;
  album_total: number;
  files_done: number;
  files_total: number;
  bytes_done: number;
  bytes_total: number;
}

export interface ShardPlacement {
  chunk_index: number;
  shard_index: number;
  storage_id: string;
  remote_key: string;
  size: number;
}

export interface BlobDetail {
  blob_hash: string;
  file_name: string;
  mime: string;
  total_len: number;
  taken_at: number;
  chunk_size: number;
  n: number;
  m: number;
  chunks: number;
  shards: ShardPlacement[];
  per_storage: [string, number][];
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
  serviceStart: () => invoke<void>("service_start"),
  serviceStop: () => invoke<void>("service_stop"),
  rebalanceAll: () => invoke<RebalanceReport>("rebalance_all"),
  importScanDir: (dir: string, createEmptyAlbums: boolean) =>
    invoke<DirScanResult>("import_scan_dir", { dir, createEmptyAlbums }),
  importPreview: (sessionId: string) => invoke<ImportPreview>("import_preview", { sessionId }),
  importExecute: (sessionId: string) => invoke<ImportRunReport>("import_execute", { sessionId }),
  importCancel: () => invoke<void>("import_cancel"),
  thumbnail: (blobHash: string) => invoke<string>("thumbnail", { blobHash }),
  blobDetail: (blobHash: string) => invoke<BlobDetail>("blob_detail", { blobHash }),
  verifyAll: () => invoke<VerifyReport[]>("verify_all"),
  importHistory: () => invoke<string[]>("import_history"),
  importReportGet: (file: string) => invoke<ImportRunReport>("import_report_get", { file }),
  getSettings: () => invoke<SettingsData>("get_settings"),
  updateSettings: (s: SettingsData) => invoke<void>("update_settings", { settings: s }),
  changePassword: (oldPw: string, newPw: string) =>
    invoke<void>("change_password", { oldPassword: oldPw, newPassword: newPw }),
  listAlbums: () => invoke<AlbumInfo[]>("list_albums"),
  albumCreate: (name: string, parentId: string | null) =>
    invoke<string>("album_create", { name, parentId }),
  albumRename: (albumId: string, name: string) =>
    invoke<void>("album_rename", { albumId, name }),
  albumSetSort: (albumId: string, sortOrder: number, parentId: string | null) =>
    invoke<void>("album_set_sort", { albumId, sortOrder, parentId }),
  albumSetCover: (albumId: string, blobHash: string) =>
    invoke<void>("album_set_cover", { albumId, blobHash }),
  thumbnailLarge: (blobHash: string) => invoke<string>("thumbnail_large", { blobHash }),
  pickDirectory: async (): Promise<string | null> => {
    const picked = await open({ directory: true, title: "选择要导入的目录" });
    return typeof picked === "string" ? picked : null;
  },

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

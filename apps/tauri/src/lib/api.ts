// HeartShard API 封装（M5b 双模式）：
// - LocalProvider：本地库（Tauri invoke，原 M1 行为）
// - RemoteProvider：连接牧心后台（fetch 后台 REST API，token 会话）
// 组件只经 `api.*` 调用，模式切换在 App.tsx 完成；`platform` 暴露能力开关。
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
  gps_lat: number | null;
  gps_lon: number | null;
  favorite: boolean;
}

export interface VerifyReport {
  blob_hash: string;
  ok: boolean;
  degraded: boolean;
  unrecoverable: boolean;
  available: number;
  missing: string[];
  corrupted: string[];
  shard_count: number;
}

export interface PreviewResult {
  path: string;
  mime: string;
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

// ---------------- 模式管理 ----------------

export type ApiMode = "local" | "remote";

const MODE_KEY = "hs_api_mode";
const REMOTE_KEY = "hs_remote_url";
const TOKEN_KEY = "hs_remote_token";

export function currentMode(): ApiMode {
  return (localStorage.getItem(MODE_KEY) as ApiMode) || "local";
}
export function setMode(mode: ApiMode) {
  localStorage.setItem(MODE_KEY, mode);
}
export function remoteUrl(): string {
  return localStorage.getItem(REMOTE_KEY) || "";
}
export function setRemoteUrl(url: string) {
  localStorage.setItem(REMOTE_KEY, url.replace(/\/$/, ""));
}
export function remoteToken(): string {
  return localStorage.getItem(TOKEN_KEY) || "";
}
export function setRemoteToken(token: string) {
  localStorage.setItem(TOKEN_KEY, token);
}
export function clearRemoteToken() {
  localStorage.removeItem(TOKEN_KEY);
}

async function remoteFetch<T>(path: string, opts: RequestInit = {}): Promise<T> {
  const headers: Record<string, string> = {
    authorization: `Bearer ${remoteToken()}`,
    ...((opts.headers as Record<string, string>) || {}),
  };
  const res = await fetch(`${remoteUrl()}${path}`, { ...opts, headers });
  if (res.status === 401) {
    clearRemoteToken();
    location.reload();
    throw new Error("登录已过期");
  }
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new Error(body.error || res.statusText);
  }
  const ct = res.headers.get("content-type") || "";
  if (!ct.includes("json")) return undefined as T;
  return res.json();
}

// 后台原图/缩略图 URL（img 标签直接用；token 走查询参数桥接）
export function remoteBlobUrl(hash: string, kind: "thumbnail" | "raw"): string {
  return `${remoteUrl()}/api/blobs/${hash}/${kind}?token=${encodeURIComponent(remoteToken())}`;
}

// ---------------- Local Provider（Tauri invoke） ----------------

const localApi = {
  mode: "local" as ApiMode,
  sessionStatus: () => invoke<SessionStatus>("session_status"),
  unlock: (password: string) => invoke<SessionStatus>("unlock", { password }),
  lock: () => invoke<SessionStatus>("lock"),
  login: undefined,
  logout: undefined,

  importFiles: (paths: string[]) => invoke<ImportOutcome[]>("import_files", { paths }),
  listBlobs: () => invoke<BlobEntry[]>("list_blobs"),
  exportBlob: (blobHash: string, destPath: string) =>
    invoke<void>("export_blob", { blobHash, destPath }),
  verifyBlob: (blobHash: string) => invoke<VerifyReport>("verify_blob", { blobHash }),
  syncNow: () => invoke<SyncOutcome>("sync_now"),
  repairBlob: (blobHash: string) => invoke<RepairReport>("repair_blob", { blobHash }),
  storageStatus: () => invoke<StorageStatus[]>("storage_status"),
  addStorageLocal: (id: string, root: string) => invoke<string>("add_storage_local", { id, root }),
  addStorageS3: (config: Record<string, unknown>) => invoke<string>("add_storage_s3", { config }),
  addStorageOpenList: (config: Record<string, unknown>) => invoke<string>("add_storage_openlist", { config }),
  addStorageWebdav: (config: Record<string, unknown>) => invoke<string>("add_storage_webdav", { config }),
  addStorageFtp: (config: Record<string, unknown>) => invoke<string>("add_storage_ftp", { config }),
  removeStorage: (id: string) => invoke<void>("remove_storage", { id }),
  previewBlob: (blobHash: string) => invoke<PreviewResult>("preview_blob", { blobHash }),
  serviceStart: () => invoke<void>("service_start"),
  serviceStop: () => invoke<void>("service_stop"),
  rebalanceAll: () => invoke<RebalanceReport>("rebalance_all"),
  importScanDir: (dir: string, createEmptyAlbums: boolean) =>
    invoke<DirScanResult>("import_scan_dir", { dir, createEmptyAlbums }),
  importPreview: (sessionId: string) => invoke<ImportPreview>("import_preview", { sessionId }),
  importExecute: (sessionId: string) => invoke<ImportRunReport>("import_execute", { sessionId }),
  importCancel: () => invoke<void>("import_cancel"),
  thumbnail: (blobHash: string) => invoke<string>("thumbnail", { blobHash }),
  thumbnailLarge: (blobHash: string) => invoke<string>("thumbnail_large", { blobHash }),
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
  albumRename: (albumId: string, name: string) => invoke<void>("album_rename", { albumId, name }),
  albumSetSort: (albumId: string, sortOrder: number, parentId: string | null) =>
    invoke<void>("album_set_sort", { albumId, sortOrder, parentId }),
  albumSetCover: (albumId: string, blobHash: string) =>
    invoke<void>("album_set_cover", { albumId, blobHash }),
  listAlbumBlobs: (albumId: string) => invoke<BlobEntry[]>("list_album_blobs", { albumId }),
  albumAddBlob: (albumId: string, blobHash: string) =>
    invoke<void>("album_add_blob", { albumId, blobHash }),
  albumRemoveBlob: (albumId: string, blobHash: string) =>
    invoke<void>("album_remove_blob", { albumId, blobHash }),
  renameBlob: (blobHash: string, newName: string) =>
    invoke<void>("rename_blob", { blobHash, newName }),
  setBlobFavorite: (blobHash: string, favorite: boolean) =>
    invoke<void>("set_blob_favorite", { blobHash, favorite }),
  deleteBlob: (blobHash: string) => invoke<void>("delete_blob", { blobHash }),
  restoreBlob: (blobHash: string) => invoke<void>("restore_blob", { blobHash }),
  purgeBlob: (blobHash: string) => invoke<void>("purge_blob", { blobHash }),
  listTrashed: () => invoke<BlobEntry[]>("list_trashed"),
  openPathSystem: (path: string) => invoke<void>("open_path_system", { path }),

  // 本地模式文件对话框
  pickFiles: async (): Promise<string[]> => {
    const picked = await open({ multiple: true, title: "选择要导入的照片 / 视频" });
    if (!picked) return [];
    return Array.isArray(picked) ? picked : [picked];
  },
  pickSavePath: async (defaultName: string): Promise<string | null> =>
    save({ title: "导出位置", defaultPath: defaultName }),

  // 缩略图 → data URL（本地解密生成）
  async thumbnailUrl(hash: string): Promise<string> {
    return invoke<string>("thumbnail", { blobHash: hash });
  },
  async largeUrl(hash: string): Promise<string> {
    return invoke<string>("thumbnail_large", { blobHash: hash });
  },
  async rawUrl(hash: string): Promise<string> {
    const r = await invoke<PreviewResult>("preview_blob", { blobHash: hash });
    return `tauri://localhost/${r.path}`.replace("tauri://localhost/", "asset://localhost/");
  },
  async uploadFiles(_files: File[], _albumId: string | null): Promise<ImportOutcome[]> {
    throw new Error("本地模式请使用「导入照片 / 视频」按钮");
  },
};

// ---------------- Remote Provider（牧心后台 REST） ----------------

const remoteApi = {
  mode: "remote" as ApiMode,
  // 后台模式：会话由 token 管理；「解锁」语义 = 登录
  sessionStatus: async (): Promise<SessionStatus> => {
    if (!remoteToken()) return { unlocked: false };
    try {
      await remoteFetch("/api/me");
      return { unlocked: true };
    } catch {
      return { unlocked: false };
    }
  },
  unlock: async (password: string): Promise<SessionStatus> => {
    // App 的 LockScreen 传的是「后台地址 用户名 口令」合并串：url|user|pass
    const [url, user, pass] = password.split("|");
    if (url) setRemoteUrl(url);
    const res = await fetch(`${remoteUrl()}/api/login`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name: user, password: pass }),
    });
    if (!res.ok) {
      const body = await res.json().catch(() => ({}));
      throw new Error(body.error || "登录失败");
    }
    const data = await res.json();
    setRemoteToken(data.token);
    return { unlocked: true };
  },
  lock: async (): Promise<SessionStatus> => {
    clearRemoteToken();
    return { unlocked: false };
  },
  login: undefined,
  logout: undefined,

  listBlobs: () => remoteFetch<BlobEntry[]>("/api/blobs"),
  exportBlob: async (blobHash: string, destPath: string) => {
    const res = await fetch(`${remoteUrl()}/api/blobs/${blobHash}/raw?token=${encodeURIComponent(remoteToken())}`);
    const buf = await res.arrayBuffer();
    await invoke<void>("export_bytes_to_file", { bytes: Array.from(new Uint8Array(buf)), destPath });
  },
  verifyBlob: async (): Promise<VerifyReport> => {
    throw new Error("后台模式不支持单 blob 体检（分片技术在后台管理）");
  },
  syncNow: () => remoteFetch("/api/sync/run", { method: "POST" }) as unknown as Promise<SyncOutcome>,
  repairBlob: async (): Promise<RepairReport> => {
    throw new Error("后台模式不支持单 blob 修复（分片技术在后台管理）");
  },
  storageStatus: () => remoteFetch<StorageStatus[]>("/api/admin/storage"),
  addStorageLocal: async (id: string, root: string) => {
    await remoteFetch("/api/admin/storage", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ kind: "local", id, root }),
    });
    return id;
  },
  addStorageS3: async (config: Record<string, unknown>) => {
    await remoteFetch("/api/admin/storage", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ kind: "s3", ...config }),
    });
    return String(config.bucket || "s3");
  },
  addStorageOpenList: async (config: Record<string, unknown>) => {
    await remoteFetch("/api/admin/storage", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ kind: "openlist", ...config }),
    });
    return String(config.endpoint || "openlist");
  },
  addStorageWebdav: async (config: Record<string, unknown>) => {
    await remoteFetch("/api/admin/storage", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ kind: "webdav", ...config }),
    });
    return String(config.endpoint || "webdav");
  },
  addStorageFtp: async (): Promise<string> => {
    throw new Error("后台暂不支持 FTPS 存储（网页端存储管理页配置）");
  },
  removeStorage: (id: string) => remoteFetch<void>(`/api/admin/storage/${id}`, { method: "DELETE" }),
  previewBlob: async (blobHash: string): Promise<PreviewResult> => {
    // 后台模式：经 Rust 下载原图到临时文件再由系统打开
    const res = await fetch(remoteBlobUrl(blobHash, "raw"));
    const buf = new Uint8Array(await res.arrayBuffer());
    const path = await invoke<string>("save_remote_preview", { bytes: Array.from(buf), blobHash });
    return { path, mime: "" };
  },
  serviceStart: async () => {},
  serviceStop: async () => {},
  rebalanceAll: async (): Promise<RebalanceReport> => {
    throw new Error("后台模式不支持再平衡（分片技术在后台管理）");
  },
  importScanDir: async (): Promise<DirScanResult> => {
    throw new Error("后台模式不支持本地目录向导（用拖拽/上传）");
  },
  importPreview: async (): Promise<ImportPreview> => {
    throw new Error("后台模式不支持本地目录向导");
  },
  importExecute: async (): Promise<ImportRunReport> => {
    throw new Error("后台模式不支持本地目录向导");
  },
  importCancel: async () => {},
  blobDetail: async (blobHash: string): Promise<BlobDetail> => {
    const blobs = await remoteFetch<BlobEntry[]>("/api/blobs");
    const b = blobs.find((x) => x.blob_hash === blobHash);
    if (!b) throw new Error("照片不在可见范围内");
    return {
      blob_hash: b.blob_hash,
      file_name: b.file_name,
      mime: b.mime,
      total_len: b.total_len,
      taken_at: b.taken_at,
      chunk_size: 0,
      n: 0,
      m: 0,
      chunks: 0,
      shards: [],
      per_storage: [],
    };
  },
  verifyAll: async (): Promise<VerifyReport[]> => {
    throw new Error("后台模式不支持一键体检（分片技术在后台管理）");
  },
  importHistory: async (): Promise<string[]> => [],
  importReportGet: async (): Promise<ImportRunReport> => {
    throw new Error("后台模式不支持导入历史");
  },
  getSettings: async (): Promise<SettingsData> => ({
    chunk_size: 0,
    n: 0,
    m: 0,
    download_concurrency: 0,
    theme: "dark",
  }),
  updateSettings: async () => {},
  changePassword: async (oldPw: string, newPw: string) => {
    await remoteFetch("/api/me/password", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ old: oldPw, new: newPw }),
    });
  },
  listAlbums: () => remoteFetch<AlbumInfo[]>("/api/albums"),
  albumCreate: async (name: string, parentId: string | null) => {
    const data = await remoteFetch<{ id: string }>("/api/albums", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name, parent_id: parentId }),
    });
    return data.id;
  },
  albumRename: async (_albumId: string, _name: string) => {
    // 后台 v1 无 album 重命名端点 → 跳过
  },
  albumSetSort: async () => {},
  albumSetCover: async () => {},
  listAlbumBlobs: (albumId: string) => remoteFetch<BlobEntry[]>(`/api/albums/${albumId}/blobs`),
  albumAddBlob: async (albumId: string, blobHash: string) => {
    await remoteFetch(`/api/albums/${albumId}/add-blobs`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ blob_hashes: [blobHash] }),
    });
  },
  albumRemoveBlob: async (albumId: string, blobHash: string) => {
    await remoteFetch(`/api/albums/${albumId}/remove-blobs`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ blob_hashes: [blobHash] }),
    });
  },
  renameBlob: (blobHash: string, newName: string) =>
    remoteFetch<void>(`/api/blobs/${blobHash}/rename`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ new_name: newName }),
    }),
  setBlobFavorite: (blobHash: string, favorite: boolean) =>
    remoteFetch<void>(`/api/blobs/${blobHash}/favorite`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ favorite }),
    }),
  deleteBlob: (blobHash: string) =>
    remoteFetch<void>(`/api/blobs/${blobHash}/delete`, { method: "POST" }),
  restoreBlob: (blobHash: string) =>
    remoteFetch<void>(`/api/blobs/${blobHash}/restore`, { method: "POST" }),
  purgeBlob: (blobHash: string) =>
    remoteFetch<void>(`/api/blobs/${blobHash}/purge`, { method: "POST" }),
  listTrashed: () => remoteFetch<BlobEntry[]>("/api/trash"),
  openPathSystem: (path: string) => invoke<void>("open_path_system", { path }),

  // 后台模式文件选择：dialog 选文件 → multipart 上传到后台
  pickFiles: async (): Promise<string[]> => {
    const picked = await open({ multiple: true, title: "选择要上传的照片 / 视频" });
    if (!picked) return [];
    return Array.isArray(picked) ? picked : [picked];
  },
  pickSavePath: async (defaultName: string): Promise<string | null> =>
    save({ title: "导出位置", defaultPath: defaultName }),

  // 缩略图/原图 → 带 token 的后台 URL
  async thumbnailUrl(hash: string): Promise<string> {
    return remoteBlobUrl(hash, "thumbnail");
  },
  async largeUrl(hash: string): Promise<string> {
    return remoteBlobUrl(hash, "raw");
  },
  async rawUrl(hash: string): Promise<string> {
    return remoteBlobUrl(hash, "raw");
  },
  async uploadFiles(files: File[], albumId: string | null): Promise<ImportOutcome[]> {
    const out: ImportOutcome[] = [];
    for (const f of files) {
      const fd = new FormData();
      fd.append("file", f, f.name);
      if (albumId) fd.append("album_id", albumId);
      try {
        const r = await remoteFetch<{ status: string; blob_hash: string | null }>("/api/blobs/upload", {
          method: "POST",
          body: fd,
        });
        out.push({ path: f.name, status: r.status as ImportStatus, blob_hash: r.blob_hash, error: null });
      } catch (e) {
        out.push({ path: f.name, status: "failed", blob_hash: null, error: String(e) });
      }
    }
    return out;
  },
};

// ---------------- 统一出口 ----------------

// 双 provider 的公共接口（编译期保证两边方法齐全）
export interface ApiProvider {
  mode: ApiMode;
  sessionStatus(): Promise<SessionStatus>;
  unlock(password: string): Promise<SessionStatus>;
  lock(): Promise<SessionStatus>;
  importFiles(paths: string[]): Promise<ImportOutcome[]>;
  listBlobs(): Promise<BlobEntry[]>;
  exportBlob(blobHash: string, destPath: string): Promise<void>;
  verifyBlob(blobHash: string): Promise<VerifyReport>;
  syncNow(): Promise<SyncOutcome>;
  repairBlob(blobHash: string): Promise<RepairReport>;
  storageStatus(): Promise<StorageStatus[]>;
  addStorageLocal(id: string, root: string): Promise<string>;
  addStorageS3(config: Record<string, unknown>): Promise<string>;
  addStorageOpenList(config: Record<string, unknown>): Promise<string>;
  addStorageWebdav(config: Record<string, unknown>): Promise<string>;
  addStorageFtp(config: Record<string, unknown>): Promise<string>;
  removeStorage(id: string): Promise<void>;
  previewBlob(blobHash: string): Promise<PreviewResult>;
  serviceStart(): Promise<void>;
  serviceStop(): Promise<void>;
  rebalanceAll(): Promise<RebalanceReport>;
  importScanDir(dir: string, createEmptyAlbums: boolean): Promise<DirScanResult>;
  importPreview(sessionId: string): Promise<ImportPreview>;
  importExecute(sessionId: string): Promise<ImportRunReport>;
  importCancel(): Promise<void>;
  thumbnail(hash: string): Promise<string>;
  thumbnailLarge(hash: string): Promise<string>;
  thumbnailUrl(hash: string): Promise<string>;
  largeUrl(hash: string): Promise<string>;
  rawUrl(hash: string): Promise<string>;
  blobDetail(blobHash: string): Promise<BlobDetail>;
  verifyAll(): Promise<VerifyReport[]>;
  importHistory(): Promise<string[]>;
  importReportGet(file: string): Promise<ImportRunReport>;
  getSettings(): Promise<SettingsData>;
  updateSettings(s: SettingsData): Promise<void>;
  changePassword(oldPw: string, newPw: string): Promise<void>;
  listAlbums(): Promise<AlbumInfo[]>;
  albumCreate(name: string, parentId: string | null): Promise<string>;
  albumRename(albumId: string, name: string): Promise<void>;
  albumSetSort(albumId: string, sortOrder: number, parentId: string | null): Promise<void>;
  albumSetCover(albumId: string, blobHash: string): Promise<void>;
  listAlbumBlobs(albumId: string): Promise<BlobEntry[]>;
  albumAddBlob(albumId: string, blobHash: string): Promise<void>;
  albumRemoveBlob(albumId: string, blobHash: string): Promise<void>;
  renameBlob(blobHash: string, newName: string): Promise<void>;
  setBlobFavorite(blobHash: string, favorite: boolean): Promise<void>;
  deleteBlob(blobHash: string): Promise<void>;
  restoreBlob(blobHash: string): Promise<void>;
  purgeBlob(blobHash: string): Promise<void>;
  listTrashed(): Promise<BlobEntry[]>;
  openPathSystem(path: string): Promise<void>;
  pickFiles(): Promise<string[]>;
  pickSavePath(defaultName: string): Promise<string | null>;
  pickDirectory(): Promise<string | null>;
  uploadFiles(files: File[], albumId: string | null): Promise<ImportOutcome[]>;
}

// 本地/远程各自缺的方法补齐为显式报错（能力由 platform 开关控制）
const remoteMissing = {
  importFiles: async (paths: string[]): Promise<ImportOutcome[]> => {
    // 把本地路径当文件读 → multipart 上传（拖拽导入路径场景）
    const out: ImportOutcome[] = [];
    for (const p of paths) {
      try {
        const bytes = await invoke<number[]>("read_local_file", { path: p });
        const name = p.split(/[\\/]/).pop() || "upload.bin";
        const file = new File([new Uint8Array(bytes)], name);
        const r = await remoteApi.uploadFiles([file], null);
        out.push(...r);
      } catch (e) {
        out.push({ path: p, status: "failed", blob_hash: null, error: String(e) });
      }
    }
    return out;
  },
  pickDirectory: async (): Promise<string | null> => {
    const picked = await open({ directory: true, title: "选择目录" });
    return typeof picked === "string" ? picked : null;
  },
};

const localMissing = {
  pickDirectory: async (): Promise<string | null> => {
    const picked = await open({ directory: true, title: "选择要导入的目录" });
    return typeof picked === "string" ? picked : null;
  },
};

export const api: ApiProvider = (() => {
  if (currentMode() === "remote") {
    return { ...remoteApi, ...remoteMissing, thumbnail: (h: string) => remoteApi.thumbnailUrl(h), thumbnailLarge: (h: string) => remoteApi.largeUrl(h) } as ApiProvider;
  }
  return { ...localApi, ...localMissing, thumbnailUrl: (h: string) => localApi.thumbnail(h), largeUrl: (h: string) => localApi.thumbnailLarge(h), rawUrl: (h: string) => localApi.previewBlob(h).then((r) => `asset://localhost/${r.path}`), uploadFiles: async () => { throw new Error("本地模式请使用「导入照片 / 视频」按钮"); } } as ApiProvider;
})();

// 能力开关：组件按此隐藏本地专属入口
export const platform = {
  isRemote: currentMode() === "remote",
  canImportWizard: currentMode() !== "remote",
  canStorageManage: currentMode() !== "remote", // 后台走网页管理页
  canRebalance: currentMode() !== "remote",
  canVerify: currentMode() !== "remote",
  canExport: true,
  canPreview: true,
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

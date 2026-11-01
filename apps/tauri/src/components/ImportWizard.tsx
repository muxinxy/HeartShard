// HeartShard 目录导入五阶段向导（§8.5）：扫描 → 预览 → 建相册+上传 → 报告。
// 进度经 `import-progress` 事件推送（相册级 + 文件级两级）。
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  api,
  formatBytes,
  ImportPreview,
  ImportProgress,
  ImportRunReport,
} from "../lib/api";

type Phase = "pick" | "preview" | "running" | "report";

export function ImportWizard({ onDone }: { onDone: () => void }) {
  const [phase, setPhase] = useState<Phase>("pick");
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [progress, setProgress] = useState<ImportProgress | null>(null);
  const [report, setReport] = useState<ImportRunReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const unlisten = listen<ImportProgress>("import-progress", (e) => {
      setProgress(e.payload);
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const pickAndScan = async () => {
    setError(null);
    const dir = await api.pickDirectory();
    if (!dir) return;
    try {
      const scan = await api.importScanDir(dir, true);
      const p = await api.importPreview(scan.session_id);
      setPreview(p);
      setPhase("preview");
    } catch (e) {
      setError(String(e));
    }
  };

  const startImport = async () => {
    if (!preview) return;
    setPhase("running");
    setError(null);
    try {
      await api.serviceStart().catch(() => {});
      const report = await api.importExecute(preview.session_id);
      setReport(report);
      setPhase("report");
    } catch (e) {
      setError(String(e));
      setPhase("pick");
    } finally {
      await api.serviceStop().catch(() => {});
    }
  };

  const cancelImport = async () => {
    await api.importCancel().catch(() => {});
  };

  const pct =
    progress && progress.files_total > 0
      ? Math.round((progress.files_done / progress.files_total) * 100)
      : 0;

  return (
    <div className="wizard">
      <header className="toolbar">
        <span className="brand">目录导入向导</span>
        <span className="spacer" />
        <button
          onClick={() => {
            onDone();
          }}
        >
          关闭
        </button>
      </header>

      {error && <div className="notice error">{error}</div>}

      {phase === "pick" && (
        <div className="empty">
          <p>选择一个目录，其层级将成为加密索引中的相册树（§8.2）。</p>
          <p className="hint">云存储上只会出现打散的对象键；目录名不会上传。</p>
          <button onClick={pickAndScan}>选择目录并扫描</button>
        </div>
      )}

      {phase === "preview" && preview && (
        <div>
          <h2>
            预览：{preview.total_files} 个文件 · {formatBytes(preview.total_bytes)} ·{" "}
            {preview.albums.length} 个相册
          </h2>
          {preview.already_in_index > 0 && (
            <p className="hint">
              其中 {preview.already_in_index} 个文件已在索引中，将跳过上传（§8.4）。
            </p>
          )}
          {preview.truncated.length > 0 && (
            <p className="hint">
              深度超限合并到第 8 层的目录：{preview.truncated.join("、")}
            </p>
          )}
          {preview.symlinks.length > 0 && (
            <p className="hint">跳过的符号链接：{preview.symlinks.length} 个</p>
          )}
          <ul className="album-list">
            {preview.albums.map((a) => (
              <li key={a.rel_path} className="blob-row">
                <span className="name">{a.rel_path || "(根目录)"}</span>
                <span className="meta">
                  {a.file_count} 文件 · {formatBytes(a.bytes)}
                </span>
              </li>
            ))}
          </ul>
          <div className="wizard-actions">
            <button className="primary" onClick={startImport}>
              开始导入
            </button>
          </div>
        </div>
      )}

      {phase === "running" && (
        <div className="running">
          <h2>
            {progress?.stage === "albums"
              ? `建相册 ${progress.album_index + 1}/${progress.album_total}：${progress.album_name ?? ""}`
              : `上传中 ${progress?.files_done ?? 0}/${progress?.files_total ?? 0}（${pct}%）`}
          </h2>
          <div className="bar">
            <div className="bar-fill" style={{ width: `${pct}%` }} />
          </div>
          {progress && (
            <p className="hint">
              {formatBytes(progress.bytes_done)} / {formatBytes(progress.bytes_total)}
            </p>
          )}
          <button onClick={cancelImport}>取消（已传文件保留）</button>
        </div>
      )}

      {phase === "report" && report && (
        <div>
          <h2>导入报告</h2>
          <ul className="report">
            <li>新建相册：{report.albums_created}（复用 {report.albums_reused}）</li>
            <li>上传成功：{report.imported}</li>
            <li>去重跳过：{report.skipped_duplicate}</li>
            <li>失败：{report.failed.length}</li>
            {report.paused && <li className="error">已暂停（连续失败 5 个）——可再次导入续传</li>}
            {report.truncated.length > 0 && <li>深度截断目录：{report.truncated.join("、")}</li>}
            {report.symlinks.length > 0 && <li>跳过符号链接：{report.symlinks.length} 个</li>}
          </ul>
          {report.failed.length > 0 && (
            <div className="notice error">
              {report.failed.map((f) => (
                <div key={f}>{f}</div>
              ))}
            </div>
          )}
          <div className="wizard-actions">
            <button
              className="primary"
              onClick={() => {
                onDone();
              }}
            >
              完成
            </button>
            <button
              onClick={() => {
                setPreview(null);
                setReport(null);
                setPhase("pick");
              }}
            >
              再导入一个目录
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

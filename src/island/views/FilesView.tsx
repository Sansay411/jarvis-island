// A dropped file and what JARVIS can do with it. The type and the action
// list come from the native side; nothing runs until a button is pressed.
// The island takes the card's measured height, so it grows with the
// result instead of clipping it.

import { useLayoutEffect, useRef } from "react";
import { Icon } from "../../ui/Icon";
import { cancelFileJob, closeFiles, resultAction, runFileAction, useFiles, type FileInfo } from "../../store/files";

const KIND: Record<string, string> = {
  image: "Изображение",
  video: "Видео",
  audio: "Аудио",
  pdf: "PDF",
  document: "Документ",
  spreadsheet: "Таблица",
  presentation: "Презентация",
  archive: "Архив",
  text: "Текст",
  code: "Код",
  folder: "Папка",
  other: "Файл",
};

function size(bytes: number): string {
  if (bytes < 1024) return `${bytes} Б`;
  const units = ["КБ", "МБ", "ГБ"];
  let v = bytes / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 ? 1 : 0).replace(".", ",")} ${units[i]}`;
}

function duration(s: number): string {
  const t = Math.round(s);
  const h = Math.floor(t / 3600);
  const m = Math.floor((t % 3600) / 60);
  const sec = String(t % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
}

function meta(f: FileInfo): string {
  const parts = [KIND[f.kind] ?? "Файл"];
  if (f.ext && f.kind !== "folder") parts[0] += ` · ${f.ext.toUpperCase()}`;
  if (f.width && f.height) parts.push(`${f.width}×${f.height}`);
  if (f.durationS) parts.push(duration(f.durationS));
  if (f.pages) parts.push(`${f.pages} стр.`);
  if (f.size) parts.push(size(f.size));
  return parts.join(" · ");
}

function many(n: number): string {
  const d = n % 10, h = n % 100;
  const w = d === 1 && h !== 11 ? "файл" : d >= 2 && d <= 4 && (h < 12 || h > 14) ? "файла" : "файлов";
  return `${n} ${w}`;
}

function Head() {
  const files = useFiles((s) => s.files);
  const paths = useFiles((s) => s.paths);
  const loading = useFiles((s) => s.loading);
  const first = files[0];
  const total = files.reduce((a, f) => a + f.size, 0);
  const name = files.length > 1 ? many(files.length) : first?.name ?? paths[0]?.split(/[\\/]/).pop() ?? "";
  const line = loading ? "Смотрю, что это…" : files.length > 1 ? `${files.map((f) => f.name).slice(0, 3).join(", ")}${files.length > 3 ? "…" : ""} · ${size(total)}` : first ? meta(first) : "";
  return (
    <div className="file-head">
      <div className={`file-thumb${first?.thumbnail ? " has-image" : ""}`}>
        {first?.thumbnail ? <img src={first.thumbnail} alt="" /> : <Icon name="file" size={20} />}
        {files.length > 1 && <span className="file-count">{files.length}</span>}
      </div>
      <div className="file-text">
        <span className="file-name" title={paths.join("\n")}>{name}</span>
        <span className="file-meta">{line}</span>
      </div>
    </div>
  );
}

function Job() {
  const job = useFiles((s) => s.job);
  if (!job) return null;
  if (job.state === "running") {
    return (
      <div className="file-job">
        <span className="job-ring" aria-hidden="true" />
        <span className="file-job-title">{job.title || "Выполняю"}</span>
        <span className="file-progress" aria-hidden="true">
          <span style={{ width: `${Math.round((job.progress ?? 0) * 100)}%` }} />
        </span>
        <button className="icon-btn" aria-label="Отменить" onClick={cancelFileJob}>
          <Icon name="stop" size={12} />
        </button>
      </div>
    );
  }
  const ok = job.state === "done";
  return (
    <div className={`file-result is-${job.state}`}>
      <div className="file-result-line">
        <Icon name={ok ? "check" : "alert"} size={14} />
        <span>{job.message}</span>
      </div>
      {job.text && <pre className="file-text-preview">{job.text}</pre>}
      {ok && job.outputs.length > 0 && (
        <div className="file-result-actions">
          <button className="btn btn-quiet" onClick={() => resultAction("open", job.outputs)}>Открыть</button>
          <button className="btn btn-quiet" onClick={() => resultAction("reveal", job.outputs)}>В папке</button>
          <button className="btn btn-quiet" onClick={() => resultAction("telegram", job.outputs)}>В Избранное</button>
          <button className="btn btn-quiet" onClick={() => resultAction("chat", job.outputs)}>В чат</button>
        </div>
      )}
    </div>
  );
}

export function FilesView() {
  const actions = useFiles((s) => s.actions);
  const error = useFiles((s) => s.error);
  const running = useFiles((s) => s.job?.state === "running");
  const ref = useRef<HTMLDivElement>(null);

  // Report the natural height so the island shape fits the card.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const report = () => useFiles.setState({ height: Math.ceil(el.scrollHeight) });
    report();
    const ro = new ResizeObserver(report);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const primary = actions.filter((a) => a.primary);
  const rest = actions.filter((a) => !a.primary);

  return (
    <div className="view view-files">
      <div ref={ref} className="files-card">
        <div className="brow">
          <span className="brow-name">Что сделать с файлом?</span>
          <div className="brow-actions">
            <button className="icon-btn" aria-label="Закрыть" onClick={closeFiles}>
              <Icon name="close" size={14} />
            </button>
          </div>
        </div>
        <div className="files-body">
          <Head />
          {error && (
            <div className="file-result is-failed">
              <div className="file-result-line">
                <Icon name="alert" size={14} />
                <span>{error}</span>
              </div>
            </div>
          )}
          {primary.length > 0 && (
            <div className="file-actions">
              {primary.map((a) => (
                <button key={a.id} className="file-action" disabled={running} title={a.hint ?? undefined} onClick={() => void runFileAction(a.id)}>
                  {a.label}
                </button>
              ))}
            </div>
          )}
          {/* Everything at once: no hidden menu to open and search through. */}
          {rest.length > 0 && (
            <div className="file-more-list">
              {rest.map((a) => (
                <button key={a.id} className="file-chip" disabled={running} title={a.hint ?? undefined} onClick={() => void runFileAction(a.id)}>
                  {a.label}
                </button>
              ))}
            </div>
          )}
          <Job />
        </div>
      </div>
    </div>
  );
}

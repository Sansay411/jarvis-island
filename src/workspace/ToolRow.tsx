import { useState } from "react";
import { Icon, type IconName } from "../ui/Icon";
import { useAgent, type ChatItem } from "../store/agent";

type ToolItem = Extract<ChatItem, { kind: "tool" }>;

function iconFor(tool: string): IconName {
  if (tool.startsWith("audio") || tool.startsWith("volume")) return "volume";
  if (tool.startsWith("brightness")) return "sun";
  if (tool.startsWith("media")) return "play";
  if (tool.startsWith("window")) return "window";
  if (tool.startsWith("app")) return "window";
  if (tool.startsWith("browser") || tool.startsWith("web") || tool === "open_url") return "globe";
  if (tool.startsWith("shell") || tool.startsWith("dev") || tool.startsWith("process")) return "terminal";
  if (tool.startsWith("screen") || tool.startsWith("ui")) return "eye";
  if (tool.startsWith("file") || tool.startsWith("fs")) return "file";
  return "bolt";
}

function StatusGlyph({ status }: { status: ToolItem["status"] }) {
  if (status === "running") return <span className="spinner" aria-label="Выполняется" />;
  if (status === "ok") return <Icon name="check" size={15} style={{ color: "var(--ok)" }} />;
  if (status === "failed") return <Icon name="alert" size={15} style={{ color: "var(--danger)" }} />;
  if (status === "declined") return <Icon name="close" size={14} style={{ color: "var(--text-3)" }} />;
  return <Icon name="eye" size={15} style={{ color: "var(--amber)" }} />;
}

export function ToolRow({ item }: { item: ToolItem }) {
  const [open, setOpen] = useState(false);
  const confirm = useAgent((s) => s.confirm);

  if (item.status === "awaiting") {
    return (
      <div className={`confirm-card risk-${item.risk ?? "moderate"}`} role="group" aria-label="Нужно подтверждение">
        <div className="confirm-head">
          <Icon name={iconFor(item.tool)} size={16} />
          <span>{item.title}</span>
        </div>
        {item.details && <pre className="confirm-details">{item.details}</pre>}
        <div className="confirm-actions">
          <button className="btn btn-quiet" onClick={() => void confirm(item.id, false)}>
            Отклонить
          </button>
          <button className="btn btn-solid" onClick={() => void confirm(item.id, true)} autoFocus>
            Разрешить
          </button>
        </div>
      </div>
    );
  }

  const hasDetails = item.args !== null || !!item.summary || !!item.error;
  return (
    <div className={`tool-row is-${item.status}`}>
      <button className="tool-line" onClick={() => hasDetails && setOpen((o) => !o)} aria-expanded={open}>
        <Icon name={iconFor(item.tool)} size={15} className="tool-icon" />
        <span className="tool-title">{item.title}</span>
        {item.summary && item.status !== "running" && <span className="tool-summary">{item.error ?? item.summary}</span>}
        <StatusGlyph status={item.status} />
      </button>
      {item.status === "running" && item.progress && item.progress.length > 0 && (
        <pre className="tool-progress">{item.progress.join("\n")}</pre>
      )}
      {open && hasDetails && (
        <div className="tool-details">
          {item.args !== null && item.args !== undefined && (
            <pre>{JSON.stringify(item.args, null, 2)}</pre>
          )}
          {(item.error || item.summary) && <p>{item.error ?? item.summary}</p>}
        </div>
      )}
    </div>
  );
}

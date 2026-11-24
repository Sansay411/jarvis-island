import { useMemo, useState } from "react";
import { Icon, type IconName } from "../ui/Icon";
import { useAgent, type ChatItem } from "../store/agent";
import { CardView } from "../overlay/cards/CardView";
import { normalizeCard } from "../overlay/cards/normalize";
import { WsIcon } from "./WorkspaceIcons";

type ToolItem = Extract<ChatItem, { kind: "tool" }>;

function iconFor(tool: string): IconName {
  if (tool === "explain_card" || tool === "explain_clear") return "cards";
  if (tool === "speak_aloud" || tool.startsWith("audio") || tool.startsWith("volume")) return "volume";
  if (tool.startsWith("brightness")) return "sun";
  if (tool.startsWith("media")) return "play";
  if (tool === "window_layout" || tool.startsWith("window") || tool.startsWith("app")) return "window";
  if (tool === "focus_session" || tool.startsWith("timer") || tool.startsWith("reminder")) return "timer";
  if (tool === "selection_grab" || tool.startsWith("clip")) return "clipboard";
  if (tool === "tasks_manage" || tool === "mission_plan") return "check";
  if (tool.startsWith("browser") || tool.startsWith("web") || tool === "open_url") return "globe";
  if (tool === "system_diagnostics" || tool.startsWith("shell") || tool.startsWith("dev") || tool.startsWith("process")) {
    return "terminal";
  }
  if (tool.startsWith("screen") || tool.startsWith("ui")) return "eye";
  if (tool.startsWith("file") || tool.startsWith("fs")) return "file";
  return "bolt";
}

function StatusGlyph({ status, hasDetails, open }: { status: ToolItem["status"]; hasDetails: boolean; open: boolean }) {
  if (status === "running") return <span className="spinner" aria-label="Выполняется" />;
  return (
    <span className="tool-status-group">
      {status === "ok" && <Icon name="check" size={14} style={{ color: "var(--ok)" }} />}
      {status === "failed" && <Icon name="alert" size={14} style={{ color: "var(--danger)" }} />}
      {status === "declined" && <Icon name="close" size={13} style={{ color: "var(--text-3)" }} />}
      {hasDetails && (
        <span className={`tool-chevron${open ? " is-open" : ""}`}>
          <WsIcon name="chevronRight" size={12} />
        </span>
      )}
    </span>
  );
}

export function ToolRow({ item }: { item: ToolItem }) {
  const [open, setOpen] = useState(false);
  const confirm = useAgent((s) => s.confirm);
  const inlineCard = useMemo(() => {
    if (item.status !== "ok") return null;
    if (item.tool !== "explain_card" && item.card === undefined) return null;
    return normalizeCard(item.card ?? item.args);
  }, [item.status, item.tool, item.card, item.args]);

  if (item.status === "awaiting") {
    const isHigh = item.risk === "high";
    return (
      <div className={`confirm-card risk-${item.risk ?? "moderate"}`} role="group" aria-label="Нужно подтверждение">
        <div className="confirm-head">
          <span className="confirm-head-left">
            <Icon name={iconFor(item.tool)} size={15} />
            <span className="confirm-title">{item.title}</span>
          </span>
          <span className={`confirm-risk-pill${isHigh ? " is-high" : ""}`}>
            <WsIcon name="shield" size={11} />
            <span>{isHigh ? "Высокий риск" : "Требует разрешения"}</span>
          </span>
        </div>
        {item.details && <pre className="confirm-details">{item.details}</pre>}
        <div className="confirm-actions">
          <button type="button" className="btn btn-quiet" onClick={() => void confirm(item.id, false)}>
            Отклонить
          </button>
          <button type="button" className="btn btn-solid confirm-allow-btn" onClick={() => void confirm(item.id, true)} autoFocus>
            <WsIcon name="check" size={13} />
            <span>Разрешить</span>
          </button>
        </div>
      </div>
    );
  }

  const hasDetails = (item.args !== null && item.args !== undefined) || !!item.summary || !!item.error;
  return (
    <div className={`tool-row is-${item.status}${open ? " is-open" : ""}`}>
      <button
        type="button"
        className="tool-line"
        onClick={() => hasDetails && setOpen((o) => !o)}
        aria-expanded={open}
      >
        <Icon name={iconFor(item.tool)} size={14} className="tool-icon" />
        <span className="tool-title">{item.title}</span>
        {item.status === "running" ? (
          <span className="tool-summary is-live">Выполняется…</span>
        ) : (
          (item.error || item.summary) && <span className="tool-summary">{item.error ?? item.summary}</span>
        )}
        <StatusGlyph status={item.status} hasDetails={hasDetails} open={open} />
      </button>
      {item.status === "running" && item.progress && item.progress.length > 0 && (
        <pre className="tool-progress">{item.progress.join("\n")}</pre>
      )}
      {open && hasDetails && (
        <div className="tool-details">
          {item.args !== null && item.args !== undefined && (
            <pre>{typeof item.args === "string" ? item.args : JSON.stringify(item.args, null, 2)}</pre>
          )}
          {(item.error || item.summary) && <p className={item.error ? "tool-detail-error" : ""}>{item.error ?? item.summary}</p>}
        </div>
      )}
      {inlineCard && (
        <div className="tool-inline-card">
          <CardView spec={inlineCard} compact />
        </div>
      )}
    </div>
  );
}

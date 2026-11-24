import type { CardSpec } from "./types";

export interface CardViewProps {
  spec: CardSpec;
  onClose: () => void;
}

export function CardView({ spec, onClose }: CardViewProps) {
  return (
    <div className="ov-card-inner">
      <div className="ov-card-head">
        <span className="ov-card-dot" aria-hidden="true" />
        <span className="ov-card-title">{spec.title ?? "JARVIS"}</span>
        <button
          type="button"
          className="ov-card-close"
          onClick={onClose}
          aria-label="Закрыть карточку"
          title="Закрыть"
        >
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
            <path d="M2.5 2.5L9.5 9.5M9.5 2.5L2.5 9.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
          </svg>
        </button>
      </div>
      {spec.body ? <p className="ov-card-body">{spec.body}</p> : null}
    </div>
  );
}

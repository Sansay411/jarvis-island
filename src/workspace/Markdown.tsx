// Minimal, safe Markdown for assistant replies: paragraphs, lists, fenced
// code, inline code, bold and links rendered as plain text. Produces React
// elements only — never raw HTML — because model output is untrusted.

import { Fragment, type ReactNode } from "react";

function inline(text: string, keyBase: string): ReactNode[] {
  const out: ReactNode[] = [];
  const re = /(`[^`]+`|\*\*[^*]+\*\*)/g;
  let last = 0;
  let m: RegExpExecArray | null;
  let i = 0;
  while ((m = re.exec(text))) {
    if (m.index > last) out.push(text.slice(last, m.index));
    const tok = m[0];
    if (tok.startsWith("`")) out.push(<code key={`${keyBase}c${i++}`}>{tok.slice(1, -1)}</code>);
    else out.push(<strong key={`${keyBase}b${i++}`}>{tok.slice(2, -2)}</strong>);
    last = m.index + tok.length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

export function Markdown({ text }: { text: string }) {
  const blocks: ReactNode[] = [];
  const parts = text.split(/```/);
  parts.forEach((part, pi) => {
    if (pi % 2 === 1) {
      const nl = part.indexOf("\n");
      const body = nl >= 0 ? part.slice(nl + 1) : part;
      blocks.push(
        <pre key={`p${pi}`} className="md-code">
          <code>{body.replace(/\n$/, "")}</code>
        </pre>,
      );
      return;
    }
    const lines = part.split("\n");
    let para: string[] = [];
    let list: { ordered: boolean; items: string[] } | null = null;
    const flushPara = () => {
      if (para.length) {
        const k = `t${pi}-${blocks.length}`;
        blocks.push(
          <p key={k}>
            {para.map((l, li) => (
              <Fragment key={li}>
                {li > 0 && <br />}
                {inline(l, `${k}-${li}`)}
              </Fragment>
            ))}
          </p>,
        );
        para = [];
      }
    };
    const flushList = () => {
      if (list) {
        const k = `l${pi}-${blocks.length}`;
        const items = list.items.map((it, ii) => <li key={ii}>{inline(it, `${k}-${ii}`)}</li>);
        blocks.push(list.ordered ? <ol key={k}>{items}</ol> : <ul key={k}>{items}</ul>);
        list = null;
      }
    };
    for (const raw of lines) {
      const line = raw.trimEnd();
      const ul = /^\s*[-*•]\s+(.*)$/.exec(line);
      const ol = /^\s*\d+[.)]\s+(.*)$/.exec(line);
      if (ul || ol) {
        flushPara();
        const ordered = !!ol;
        if (!list || list.ordered !== ordered) {
          flushList();
          list = { ordered, items: [] };
        }
        list.items.push((ul ?? ol)![1] ?? "");
      } else if (line.trim() === "") {
        flushPara();
        flushList();
      } else {
        flushList();
        para.push(line.replace(/^#{1,6}\s+/, ""));
      }
    }
    flushPara();
    flushList();
  });
  return <div className="md">{blocks}</div>;
}

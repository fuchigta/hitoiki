import type { ComponentChildren } from "preact";

// 簡易 Markdown 表示。見出し・箇条書き（番号付き含む）・段落・コードブロック・太字・インラインコードだけを扱う。
// 文字列は常にテキストノードとして描画する（HTML は解釈されず、そのまま文字として表示される）。

type Block =
  | { t: "h"; level: number; text: string }
  | { t: "ul"; items: string[] }
  | { t: "ol"; items: string[] }
  | { t: "p"; text: string }
  | { t: "code"; text: string };

export function parseMarkdown(src: string): Block[] {
  const lines = src.replace(/\r\n?/g, "\n").split("\n");
  const out: Block[] = [];
  let para: string[] = [];
  const flush = () => {
    if (para.length) out.push({ t: "p", text: para.join("\n") });
    para = [];
  };
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (/^\s*```/.test(line)) {
      flush();
      const code: string[] = [];
      i++;
      while (i < lines.length && !/^\s*```/.test(lines[i])) code.push(lines[i++]);
      out.push({ t: "code", text: code.join("\n") });
      continue;
    }
    const h = /^(#{1,6})\s+(.*)$/.exec(line);
    if (h) {
      flush();
      out.push({ t: "h", level: h[1].length, text: h[2].trim() });
      continue;
    }
    const ul = /^\s*[-*+]\s+(.*)$/.exec(line);
    const ol = /^\s*\d+[.)]\s+(.*)$/.exec(line);
    if (ul || ol) {
      flush();
      const kind = ul ? "ul" : "ol";
      const last = out[out.length - 1];
      if (last && last.t === kind) last.items.push((ul ?? ol)![1]);
      else out.push({ t: kind, items: [(ul ?? ol)![1]] });
      continue;
    }
    if (line.trim() === "") {
      flush();
      continue;
    }
    // 箇条書きの続き行（インデントされた行）は直前の項目に足す。
    const last = out[out.length - 1];
    if (para.length === 0 && /^\s+\S/.test(line) && last && (last.t === "ul" || last.t === "ol")) {
      last.items[last.items.length - 1] += " " + line.trim();
      continue;
    }
    para.push(line);
  }
  flush();
  return out;
}

/** `**太字**` と `` `コード` `` だけをインライン変換する。 */
function inline(text: string): ComponentChildren {
  const parts: ComponentChildren[] = [];
  const re = /\*\*([^*]+)\*\*|`([^`]+)`/g;
  let last = 0;
  let m: RegExpExecArray | null;
  let k = 0;
  while ((m = re.exec(text))) {
    if (m.index > last) parts.push(text.slice(last, m.index));
    parts.push(m[1] !== undefined ? <strong key={k++}>{m[1]}</strong> : <code key={k++}>{m[2]}</code>);
    last = m.index + m[0].length;
  }
  if (last < text.length) parts.push(text.slice(last));
  return parts;
}

export function Markdown({ text }: { text: string }) {
  const blocks = parseMarkdown(text);
  return (
    <div class="md">
      {blocks.map((b, i) => {
        switch (b.t) {
          case "h": {
            // ページ見出し(h1)と区別するため、本文中の見出しは h3 以下にそろえる。
            const Tag = (`h${Math.min(6, Math.max(3, b.level + 1))}` as unknown) as "h3";
            return <Tag key={i}>{inline(b.text)}</Tag>;
          }
          case "ul":
            return (
              <ul key={i}>
                {b.items.map((it, j) => (
                  <li key={j}>{inline(it)}</li>
                ))}
              </ul>
            );
          case "ol":
            return (
              <ol key={i}>
                {b.items.map((it, j) => (
                  <li key={j}>{inline(it)}</li>
                ))}
              </ol>
            );
          case "code":
            return (
              <pre key={i}>
                <code>{b.text}</code>
              </pre>
            );
          default:
            return <p key={i}>{inline(b.text)}</p>;
        }
      })}
    </div>
  );
}

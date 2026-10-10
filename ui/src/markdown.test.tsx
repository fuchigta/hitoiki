import { describe, expect, it } from "vitest";
import type { ComponentChildren, VNode } from "preact";
import { Markdown, parseMarkdown } from "./markdown";

/** vnode を (タグ名, 子) の木にして文字列化する。文字列は常にテキストとして扱われる。 */
function dump(node: ComponentChildren): string {
  if (node == null || typeof node === "boolean") return "";
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(dump).join("");
  const v = node as VNode<{ children?: ComponentChildren; dangerouslySetInnerHTML?: unknown }>;
  expect(v.props.dangerouslySetInnerHTML).toBeUndefined();
  return `<${String(v.type)}>${dump(v.props.children)}</${String(v.type)}>`;
}

describe("parseMarkdown", () => {
  it("見出し・箇条書き・番号付き・コード・段落", () => {
    const b = parseMarkdown("# 題\n\n- a\n- b\n\n1. x\n2. y\n\n```\ncode <b>\n```\n\n本文1\n本文2");
    expect(b).toEqual([
      { t: "h", level: 1, text: "題" },
      { t: "ul", items: ["a", "b"] },
      { t: "ol", items: ["x", "y"] },
      { t: "code", text: "code <b>" },
      { t: "p", text: "本文1\n本文2" },
    ]);
  });

  it("箇条書きの続き行は直前の項目に連結する", () => {
    expect(parseMarkdown("- a\n  続き")).toEqual([{ t: "ul", items: ["a 続き"] }]);
  });

  it("CRLF を扱える", () => {
    expect(parseMarkdown("# a\r\n- b")).toEqual([
      { t: "h", level: 1, text: "a" },
      { t: "ul", items: ["b"] },
    ]);
  });
});

describe("Markdown の描画", () => {
  it("HTML はエスケープされずテキストのまま渡る（innerHTML を使わない）", () => {
    const out = dump(Markdown({ text: "<script>alert(1)</script>\n\n- <img src=x onerror=1>" }));
    expect(out).toContain("<p><script>alert(1)</script></p>");
    expect(out).toContain("<li><img src=x onerror=1></li>");
  });

  it("見出しは h3 以下、太字とインラインコードを変換する", () => {
    const out = dump(Markdown({ text: "# 題\n\n**強**と`code`" }));
    expect(out).toContain("<h3>題</h3>");
    expect(out).toContain("<strong>強</strong>");
    expect(out).toContain("<code>code</code>");
  });

  it("コードブロックは pre/code", () => {
    expect(dump(Markdown({ text: "```\na <b> c\n```" }))).toContain("<pre><code>a <b> c</code></pre>");
  });
});

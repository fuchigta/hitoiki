// audit.mjs がページの中で実行する関数群。page.evaluate に関数として渡すので、各関数は外の変数に頼らず自己完結させる。
// 開発専用。使い方は docs/dev/development.md の「UI の細かい不統一の検査」。

/** 切り替え系の部品の候補を、文書順に列挙する（再読み込み後も同じ順序になる）。 */
export function listToggles() {
  const SEL =
    '[role=tab], button.chip, button[role=radio], button[aria-pressed]:not(.object-main):not(.hm-cell), input[type=radio], input[type=checkbox], select';
  const visible = (el) => {
    const r = el.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) return false;
    if (el.closest('[role=menu], [role=tooltip], .help-pop, .nav')) return false;
    return !el.disabled;
  };
  const describe = (el) => {
    const label = el.getAttribute("aria-label") || (el.labels?.[0]?.textContent ?? "") || el.textContent || "";
    return `${el.tagName.toLowerCase()}${el.getAttribute("role") ? `[${el.getAttribute("role")}]` : ""}${el.type && el.tagName === "INPUT" ? `[${el.type}]` : ""}:${label.trim().replace(/\s+/g, " ").slice(0, 24)}`;
  };
  const out = [];
  const DANGER = /削除|保存|確定|無視|取り込|送信|作成|追加|実行|消去|リセット/;
  [...document.querySelectorAll(SEL)].forEach((el, index) => {
    if (!visible(el)) return;
    const text = (el.textContent || "").trim();
    if (el.tagName === "BUTTON" && DANGER.test(text)) return;
    if (el.tagName === "SELECT") {
      const n = el.options.length;
      const cur = el.selectedIndex;
      for (let i = 0; i < Math.min(n, 6); i++) {
        if (i === cur) continue;
        out.push({ index, option: i, name: `${describe(el)} → ${el.options[i].textContent.trim().slice(0, 20)}` });
      }
      return;
    }
    out.push({ index, option: null, name: describe(el) });
  });
  return { SEL, list: out };
}

/** 監視対象の要素に data-au を振り、位置と大きさを返す。 */
export function tagAll() {
  const scrollers = [];
  const scrollerOf = (el) => {
    for (let n = el.parentElement; n && n !== document.body; n = n.parentElement) {
      if (n.classList.contains("content")) return -1; // 本文のスクロールは y の補正で吸収する
      const oy = getComputedStyle(n).overflowY;
      if ((oy === "auto" || oy === "scroll") && n.scrollHeight > n.clientHeight) {
        let i = scrollers.indexOf(n);
        if (i < 0) i = scrollers.push(n) - 1;
        n.setAttribute("data-au-sc", String(i));
        return i;
      }
    }
    return -1;
  };
  const WATCH =
    "button, input, select, textarea, label, h1, h2, h3, h4, [role=tab], .tag, .chip, .meta, .card, .row, .content > *, a, .toolbar, .object-row, .field";
  const content = document.querySelector(".content");
  const els = [...document.querySelectorAll(WATCH)].filter((el) => {
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0 && !el.closest('[role=menu], [role=tooltip], .help-pop');
  });
  const out = [];
  els.forEach((el, n) => {
    el.setAttribute("data-au", String(n));
    const r = el.getBoundingClientRect();
    const inContent = content?.contains(el);
    const dy = inContent ? content.scrollTop : 0;
    const text = (el.getAttribute("aria-label") || el.textContent || "").trim().replace(/\s+/g, " ").slice(0, 20);
    out.push({
      n,
      tag: el.tagName.toLowerCase(),
      cls: (el.getAttribute("class") || "").split(/\s+/).filter(Boolean).slice(0, 3).join("."),
      text,
      x: r.left,
      y: r.top + dy,
      w: r.width,
      h: r.height,
      sc: scrollerOf(el),
      sticky: (() => { for (let n = el; n && n !== document.body; n = n.parentElement) if (getComputedStyle(n).position === "sticky") return true; return false; })(),
      ctl: /^(BUTTON|INPUT|SELECT|TEXTAREA)$/.test(el.tagName) || el.getAttribute("role") === "tab" || el.classList.contains("chip"),
    });
  });
  return { cw: content?.clientWidth ?? 0, sy: content?.scrollTop ?? 0, sc: scrollers.map((n) => n.scrollTop), els: out };
}

/** 操作の対象を data-au-p で目印にする。 */
export function markPressed({ SEL, index }) {
  const el = document.querySelectorAll(SEL)[index];
  if (!el) return false;
  el.setAttribute("data-au-p", "1");
  return true;
}

/** tagAll の後、同じ data-au の要素の位置を取り直す。押した部品の祖先の印も返す。 */
export function measureAgain() {
  const content = document.querySelector(".content");
  const pressed = document.querySelector("[data-au-p]");
  const out = [];
  for (const el of document.querySelectorAll("[data-au]")) {
    const r = el.getBoundingClientRect();
    const inContent = content?.contains(el);
    const dy = inContent ? content.scrollTop : 0;
    out.push({
      n: Number(el.getAttribute("data-au")),
      x: r.left,
      y: r.top + dy,
      w: r.width,
      h: r.height,
      vis: r.width > 0 && r.height > 0,
      anc: !!pressed && el.contains(pressed) && el !== pressed,
      pressed: el === pressed,
    });
  }
  const sc = [];
  for (const n of document.querySelectorAll("[data-au-sc]")) sc[Number(n.getAttribute("data-au-sc"))] = n.scrollTop;
  return { cw: content?.clientWidth ?? 0, sy: content?.scrollTop ?? 0, sc, els: out };
}

/** 中身が入れ物の幅をはみ出している要素（横スクロールや、はみ出しの見切れ）を返す。 */
export function findOverflow() {
  const out = [];
  for (const el of document.querySelectorAll("body *")) {
    if (el.closest("svg")) continue;
    const r = el.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) continue;
    const st = getComputedStyle(el);
    // 意図した省略（text-overflow: ellipsis）と、横スクロールにしてある入れ物（とその中身）は除く。
    if (st.textOverflow === "ellipsis") continue;
    if (st.overflowX === "auto" || st.overflowX === "scroll") continue;
    let inScroller = false;
    for (let n = el.parentElement; n; n = n.parentElement) {
      const ox = getComputedStyle(n).overflowX;
      if (ox === "auto" || ox === "scroll") inScroller = true;
    }
    if (inScroller && el.closest(".heatmap-scroll")) continue;
    if (el.scrollWidth > el.clientWidth + 1 && el.clientWidth > 0) {
      const cls = (el.getAttribute("class") || "").split(/\s+/).filter(Boolean).slice(0, 3).join(".");
      out.push({ id: `${el.tagName.toLowerCase()}${cls ? `.${cls}` : ""}`, over: el.scrollWidth - el.clientWidth, ox: getComputedStyle(el).overflowX });
    }
  }
  return out;
}

/** 全部の要素のスタイルを集めて返す（部品の種類ごとの値と、トークン外の値の素材）。 */
export function collectStyles() {
  const root = document.documentElement;
  // トークンの色を引く（:root の --* のうち、色として解釈できるもの）。
  const varNames = new Set();
  for (const sheet of document.styleSheets) {
    let rules;
    try {
      rules = sheet.cssRules;
    } catch {
      continue;
    }
    const walk = (list) => {
      for (const r of list) {
        if (r.cssRules && !r.selectorText) walk(r.cssRules);
        else if (r.selectorText === ":root") for (const p of r.style) if (p.startsWith("--")) varNames.add(p);
      }
    };
    walk(rules);
  }
  const probe = document.createElement("span");
  document.body.appendChild(probe);
  const tokenColors = [];
  const cs = getComputedStyle(root);
  for (const v of varNames) {
    if (!/^--(bg|surface|text|muted|border|accent|warn|k-)/.test(v)) continue;
    probe.style.color = `var(${v})`;
    tokenColors.push(getComputedStyle(probe).color);
  }
  probe.remove();
  const px = (name) => parseFloat(cs.getPropertyValue(name));
  const tokens = {
    fs: ["--fs-xs", "--fs-s", "--fs-m", "--fs-l", "--fs-xl", "--fs-num"].map(px),
    sp: [0, ...["--sp-1", "--sp-2", "--sp-3", "--sp-4", "--sp-5", "--sp-6", "--sp-7"].map(px)],
    r: [0, ...["--r-s", "--r-m", "--r-l"].map(px)],
  };

  const variantClasses = ["tab", "vtl-del", "ghost", "primary", "chip", "link", "help-btn", "menu-btn", "menu-item", "object-main", "hm-cell", "attn"];
  const kindOf = (el) => {
    const t = el.tagName.toLowerCase();
    if (t === "button") {
      const v = variantClasses.filter((c) => el.classList.contains(c)).filter((c) => c !== "attn");
      return v.length ? `button.${v.join(".")}` : "button";
    }
    if (t === "input") return `input[${el.type}]`;
    if (["select", "textarea", "h1", "h2", "h3", "h4"].includes(t)) return t;
    if (el.classList.contains("tag")) return "tag";
    return null;
  };
  const describe = (el) => {
    const cls = (el.getAttribute("class") || "").split(/\s+/).filter(Boolean).slice(0, 3).join(".");
    const text = (el.getAttribute("aria-label") || el.value || el.textContent || "").toString().trim().replace(/\s+/g, " ").slice(0, 18);
    return `${el.tagName.toLowerCase()}${cls ? `.${cls}` : ""}${text ? `「${text}」` : ""}`;
  };
  const visible = (el) => {
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  };
  const parseColor = (s) => {
    let m = /^rgba?\(([^)]+)\)$/.exec(s);
    if (m) {
      const p = m[1].split(/[ ,/]+/).filter(Boolean).map(Number);
      return { rgb: p.slice(0, 3), a: p.length > 3 ? p[3] : 1 };
    }
    m = /^color\(srgb ([^)]+)\)$/.exec(s);
    if (m) {
      const p = m[1].split(/[ /]+/).filter(Boolean).map(Number);
      return { rgb: p.slice(0, 3).map((v) => v * 255), a: p.length > 3 ? p[3] : 1 };
    }
    return null;
  };
  const tokenRgb = tokenColors.map(parseColor).filter(Boolean);
  const isTokenColor = (s) => {
    const c = parseColor(s);
    if (!c) return true; // 解釈できない形式は判定しない
    if (c.a === 0) return true;
    return tokenRgb.some((t) => t.rgb.every((v, i) => Math.abs(v - c.rgb[i]) <= 1.5));
  };
  const near = (v, list) => list.some((t) => Math.abs(t - v) < 0.01);

  const kinds = []; // {kind, h, fs, pad, radius, desc}
  const off = new Map(); // key -> {count, sample}
  const addOff = (key, el) => {
    const e = off.get(key);
    if (e) e.count += 1;
    else off.set(key, { count: 1, sample: describe(el) });
  };
  for (const el of document.querySelectorAll("body *")) {
    if (el.closest("svg") || el.tagName === "SCRIPT" || el.tagName === "STYLE" || !visible(el)) continue;
    const st = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    const kind = kindOf(el);
    if (kind) {
      kinds.push({
        kind,
        h: st.display === "inline" ? null : Math.round(r.height), // inline は文字の箱の高さで、見た目の高さではない
        fs: st.fontSize,
        fw: st.fontWeight,
        pad: `${st.paddingTop} ${st.paddingRight}`,
        radius: st.borderTopLeftRadius,
        mb: /^h\d$/.test(el.tagName.toLowerCase()) ? `${st.marginTop}/${st.marginBottom}` : "",
        desc: describe(el),
      });
    }
    // 文字を直接持つ要素だけ font-size を見る（空のラッパーの継承値は除く）。
    const hasText = [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim());
    if (hasText) {
      const fs = parseFloat(st.fontSize);
      if (!near(fs, tokens.fs)) addOff(`font-size|${st.fontSize}`, el);
      if (!isTokenColor(st.color)) addOff(`color|${st.color}`, el);
    }
    for (const side of ["Top", "Right", "Bottom", "Left"]) {
      const p = parseFloat(st[`padding${side}`]);
      if (p && !near(p, tokens.sp)) addOff(`padding|${p}px`, el);
    }
    for (const prop of ["rowGap", "columnGap"]) {
      const g = parseFloat(st[prop]);
      if (g && !near(g, tokens.sp)) addOff(`gap|${g}px`, el);
    }
    for (const corner of ["TopLeft", "TopRight", "BottomRight", "BottomLeft"]) {
      const v = st[`border${corner}Radius`];
      if (v.endsWith("%")) continue;
      const n = parseFloat(v);
      if (n && n < 900 && !near(n, tokens.r)) addOff(`radius|${v}`, el);
    }
    if (!isTokenColor(st.backgroundColor)) addOff(`background|${st.backgroundColor}`, el);
    if (parseFloat(st.borderTopWidth) > 0 && st.borderTopStyle !== "none" && !isTokenColor(st.borderTopColor)) addOff(`border-color|${st.borderTopColor}`, el);
  }

  // 横並びの操作部品の高さ・垂直中心の不揃い。
  const ctlSel = "button, select, input:not([type=checkbox]):not([type=radio]), label.check, [role=tab]";
  const ctls = [...document.querySelectorAll(ctlSel)].filter(
    (el) => visible(el) && !el.closest('[role=menu], [role=tooltip], .help-pop, svg, .hm-body, .heatmap') && !el.classList.contains("object-main") && !el.classList.contains("hm-cell"),
  );
  const lca = (a, b) => {
    const set = new Set();
    for (let n = a; n; n = n.parentElement) set.add(n);
    for (let n = b; n; n = n.parentElement) if (set.has(n)) return n;
    return document.body;
  };
  const rows = [];
  for (let i = 0; i < ctls.length; i++) {
    for (let j = i + 1; j < ctls.length; j++) {
      const a = ctls[i];
      const b = ctls[j];
      if (a.contains(b) || b.contains(a)) continue;
      const ra = a.getBoundingClientRect();
      const rb = b.getBoundingClientRect();
      const overlap = Math.min(ra.bottom, rb.bottom) - Math.max(ra.top, rb.top);
      if (overlap < 0.5 * Math.min(ra.height, rb.height)) continue; // 同じ行ではない
      if (ra.right > rb.left + 0.5 && rb.right > ra.left + 0.5) continue; // 横に並んでいない
      const anc = lca(a, b);
      const ar = anc.getBoundingClientRect();
      if (ar.height > Math.max(ra.height, rb.height) * 1.6 + 8) continue; // 1 行の入れ物ではない
      const ha = Math.round(ra.height * 2) / 2;
      const hb = Math.round(rb.height * 2) / 2;
      const ca = ra.top + ra.height / 2;
      const cb = rb.top + rb.height / 2;
      const isLabel = a.tagName === "LABEL" || b.tagName === "LABEL";
      const dh = isLabel ? 0 : Math.abs(ha - hb);
      const dc = Math.abs(ca - cb);
      if (dh >= 1 || dc >= 1) rows.push({ a: describe(a), b: describe(b), ha, hb, dc: Math.round(dc * 2) / 2 });
    }
  }
  return { kinds, off: [...off.entries()].map(([key, v]) => ({ key, ...v })), rows, tokens };
}

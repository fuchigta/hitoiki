import type { ComponentChildren } from "preact";
import { useEffect, useId, useRef, useState } from "preact/hooks";
import { HELP_CLOSED, helpEscape, helpOpen, helpToggle, type HelpState } from "./help-util";
import { firstEnabled, menuKey } from "./menu-util";
import { periodOptions, type PeriodId } from "./period-util";
import { SCALE_VALUES, stepScale, toggleScale } from "./scale-util";
import { stepTab } from "./tabs-util";

// 画面をまたいで使う小さな部品。見た目のトークンは styles.css の `:root` にある。

// ===== Help =====

/** 「?」の小さなボタン。押す・hover・focus で吹き出しに説明文を出す。Esc で閉じる。常時表示の長い説明文はここへ移す。 */
export function Help({ children, label = "説明" }: { children: ComponentChildren; label?: string }) {
  const id = useId();
  const [st, setSt] = useState<HelpState>(HELP_CLOSED);
  const open = helpOpen(st);
  return (
    <span class="help" onMouseEnter={() => setSt((s) => ({ ...s, hover: true }))} onMouseLeave={() => setSt((s) => ({ ...s, hover: false }))}>
      <button
        type="button"
        class="ghost help-btn"
        aria-label={label}
        aria-expanded={open}
        aria-describedby={open ? id : undefined}
        onClick={() => setSt((s) => helpToggle(s))}
        onFocus={() => setSt((s) => ({ ...s, focus: true }))}
        onBlur={() => setSt((s) => ({ ...s, focus: false, pinned: false }))}
        onKeyDown={(e) => {
          if (e.key === "Escape" && open) {
            e.stopPropagation();
            setSt(helpEscape());
          }
        }}
      >
        ?
      </button>
      {open && (
        <span id={id} role="tooltip" class="help-pop">
          {children}
        </span>
      )}
    </span>
  );
}

// ===== OverflowMenu =====

export interface MenuItem {
  label: string;
  onSelect: () => void;
  disabled?: boolean;
}

/** 「…」ボタンから開くメニュー。↑↓ で移動、Enter で選択、Esc と外側クリックで閉じる。 */
export function OverflowMenu({ items, label = "操作" }: { items: MenuItem[]; label?: string }) {
  const [open, setOpen] = useState(false);
  const [cur, setCur] = useState(-1);
  const wrap = useRef<HTMLSpanElement>(null);
  const btn = useRef<HTMLButtonElement>(null);
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const menuId = useId();

  useEffect(() => {
    if (!open) return;
    const h = (e: MouseEvent) => {
      if (wrap.current && !wrap.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [open]);

  useEffect(() => {
    if (open && cur >= 0) refs.current[cur]?.focus();
  }, [open, cur]);

  const close = (focusBack: boolean) => {
    setOpen(false);
    setCur(-1);
    if (focusBack) btn.current?.focus();
  };
  const openMenu = () => {
    setOpen(true);
    setCur(firstEnabled(items));
  };
  const select = (i: number) => {
    const it = items[i];
    if (!it || it.disabled) return;
    close(true);
    it.onSelect();
  };

  return (
    <span class="menu-wrap" ref={wrap}>
      <button
        ref={btn}
        type="button"
        class="ghost menu-btn"
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        onClick={() => (open ? close(false) : openMenu())}
        onKeyDown={(e) => {
          if (!open && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
            e.preventDefault();
            openMenu();
          } else if (open && e.key === "Escape") {
            e.stopPropagation();
            close(true);
          }
        }}
      >
        …
      </button>
      {open && (
        <div
          id={menuId}
          class="menu"
          role="menu"
          aria-label={label}
          onKeyDown={(e) => {
            if (e.key === "Tab") {
              close(false);
              return;
            }
            const a = menuKey(items, cur, e.key);
            if (!a) return;
            e.preventDefault();
            e.stopPropagation();
            if (a.type === "move") {
              if (a.to >= 0) setCur(a.to);
            } else if (a.type === "close") close(true);
            else select(a.at);
          }}
        >
          {items.map((it, i) => (
            <button
              key={it.label}
              ref={(el) => {
                refs.current[i] = el;
              }}
              type="button"
              role="menuitem"
              class="menu-item"
              tabIndex={i === cur ? 0 : -1}
              disabled={it.disabled}
              onClick={() => select(i)}
              onMouseEnter={() => !it.disabled && setCur(i)}
            >
              {it.label}
            </button>
          ))}
        </div>
      )}
    </span>
  );
}

// ===== ListDetail / ObjectList =====

/** 左に一覧、右に詳細の 2 カラム。狭いときは 1 カラムに縦積みする。 */
export function ListDetail({ list, detail }: { list: ComponentChildren; detail: ComponentChildren }) {
  return (
    <div class="list-detail">
      <div class="ld-list">{list}</div>
      <div class="ld-detail">{detail}</div>
    </div>
  );
}

/** 一覧（`list` + 行ごとの選択ボタン）。 */
export function ObjectList({ children, label }: { children: ComponentChildren; label?: string }) {
  return (
    <ul class="object-list" aria-label={label}>
      {children}
    </ul>
  );
}

export interface ObjectRowProps {
  title: ComponentChildren;
  /** 付随情報（時刻・件数・状態など）。 */
  meta?: ComponentChildren;
  /** 表示専用のラベル（押せない）。 */
  tags?: string[];
  selected?: boolean;
  onSelect: () => void;
  /** 行末の「…」メニュー。 */
  menu?: MenuItem[];
  menuLabel?: string;
  /** 行末に置く追加の要素（メニューの手前）。 */
  trailing?: ComponentChildren;
  /** 階層のインデント（0〜3）。 */
  indent?: number;
  /** 見出し行（Objective など、配下の行をまとめるもの）。 */
  group?: boolean;
}

/** 一覧の 1 行。行全体のクリックで選択し、行末に OverflowMenu を置く。 */
export function ObjectRow({ title, meta, tags, selected, onSelect, menu, menuLabel, trailing, indent, group }: ObjectRowProps) {
  const cls = ["object-row", selected ? "sel" : "", indent ? `indent-${Math.min(indent, 3)}` : "", group ? "group" : ""]
    .filter(Boolean)
    .join(" ");
  return (
    <li class={cls}>
      <button type="button" class="object-main" aria-pressed={!!selected} onClick={onSelect}>
        <span class="object-title">{title}</span>
        {tags?.map((t) => (
          <span key={t} class="tag">
            {t}
          </span>
        ))}
        {meta != null && meta !== false && <span class="meta object-meta">{meta}</span>}
      </button>
      {trailing}
      {menu && menu.length > 0 && <OverflowMenu items={menu} label={menuLabel} />}
    </li>
  );
}

// ===== Tabs =====

export interface TabDef {
  id: string;
  label: string;
}

/**
 * タブの見出し（role="tablist"）。←→ Home End で動く。中身は `TabPanel` を並べて渡す。
 * `idBase` はタブと中身を結ぶ ID の接頭辞（画面内で一意に）。
 */
export function Tabs({
  tabs,
  active,
  onChange,
  idBase,
  label,
  panels = true,
}: {
  tabs: TabDef[];
  active: string;
  onChange: (id: string) => void;
  idBase: string;
  label?: string;
  /** 中身を `TabPanel` で並べるか。false なら、タブは一覧の切り替えだけに使う（aria-controls を付けない）。 */
  panels?: boolean;
}) {
  const refs = useRef<Record<string, HTMLButtonElement | null>>({});
  const ids = tabs.map((t) => t.id);
  return (
    <div class="tabs" role="tablist" aria-label={label}>
      {tabs.map((t) => (
        <button
          key={t.id}
          ref={(el) => {
            refs.current[t.id] = el;
          }}
          type="button"
          role="tab"
          id={`${idBase}-tab-${t.id}`}
          class="tab"
          aria-selected={active === t.id}
          aria-controls={panels ? `${idBase}-panel-${t.id}` : undefined}
          tabIndex={active === t.id ? 0 : -1}
          onClick={() => onChange(t.id)}
          onKeyDown={(e) => {
            const next = stepTab(ids, active, e.key);
            if (next === undefined) return;
            e.preventDefault();
            onChange(next);
            refs.current[next]?.focus();
          }}
        >
          {t.label}
        </button>
      ))}
    </div>
  );
}

/** タブの中身。選ばれていないものは隠すだけで、アンマウントしない（編集中のフォームを保つ）。 */
export function TabPanel({
  id,
  idBase,
  active,
  children,
}: {
  id: string;
  idBase: string;
  active: string;
  children: ComponentChildren;
}) {
  return (
    <div
      class="tab-panel"
      role="tabpanel"
      id={`${idBase}-panel-${id}`}
      aria-labelledby={`${idBase}-tab-${id}`}
      hidden={active !== id}
    >
      {children}
    </div>
  );
}

// ===== PeriodSelect =====

/** 期間の選択（今日・昨日・今週・先週）。画面ごとに使える選択肢を `only` で絞る。 */
export function PeriodSelect({
  value,
  onChange,
  only,
}: {
  value: string;
  onChange: (p: PeriodId) => void;
  only?: PeriodId[];
}) {
  return (
    <select value={value} onChange={(e) => onChange(e.currentTarget.value as PeriodId)} aria-label="期間">
      {periodOptions(only).map((p) => (
        <option key={p.id} value={p.id}>
          {p.label}
        </option>
      ))}
    </select>
  );
}

// ===== Scale =====

/** 1〜5 の選択。もう一度押すと外れる。←→ で動かす（radiogroup）。 */
export function Scale({
  value,
  onChange,
  label,
}: {
  value?: number;
  onChange: (n: number | undefined) => void;
  label: string;
}) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  // roving tabindex: 選択中（なければ先頭）だけが Tab で止まる。
  const tabbable = value ?? SCALE_VALUES[0];
  return (
    <div class="field">
      <span>{label}</span>
      <div class="scale" role="radiogroup" aria-label={label}>
        {SCALE_VALUES.map((n, i) => (
          <button
            type="button"
            key={n}
            ref={(el) => {
              refs.current[i] = el;
            }}
            role="radio"
            aria-checked={value === n}
            tabIndex={n === tabbable ? 0 : -1}
            class={value === n ? "chip on" : "chip"}
            onClick={() => onChange(toggleScale(value, n))}
            onKeyDown={(e) => {
              const next = stepScale(value, e.key);
              if (next === undefined) return;
              e.preventDefault();
              onChange(next);
              refs.current[next - 1]?.focus();
            }}
          >
            {n}
          </button>
        ))}
      </div>
    </div>
  );
}

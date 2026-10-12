import { useId, useRef, useState } from "preact/hooks";
import { filterKrs, krLabel, type KrOption } from "./kr-util";
import { moveIndex } from "./task-picker-util";

interface Props {
  /** 選択中の KR の ID（無ければ空）。 */
  value: string;
  /** 候補の KR（未取得のうちは空でよい）。 */
  options: KrOption[];
  onChange: (id: string) => void;
  ariaLabel: string;
  disabled?: boolean;
}

/**
 * KR を 1 つ選ぶ入力欄。選んだ KR は「Objective / KR」のチップで見せ、× で外せる。
 * 入力で KR 名・Objective 名を部分一致で絞り込む。
 * キーボード: ↑↓ で移動、Enter / Tab で選択、Backspace（入力が空のとき）で外す、Esc で閉じる。
 */
export function KrPicker(p: Props) {
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listId = useId();

  const selected = p.options.find((o) => o.id === p.value);
  const items = filterKrs(p.options, query);
  const listOpen = open && items.length > 0;
  const idx = items.length === 0 ? -1 : Math.min(active, items.length - 1);
  const noMatch = open && query.trim() !== "" && items.length === 0;

  const pick = (o: KrOption) => {
    p.onChange(o.id);
    setQuery("");
    setOpen(false);
    setActive(0);
  };
  const clear = () => {
    p.onChange("");
    setQuery("");
    inputRef.current?.focus();
  };

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.isComposing) return; // IME の変換確定中は触らない
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!listOpen) {
        setOpen(true);
        setActive(0);
        return;
      }
      setActive(moveIndex(idx, e.key === "ArrowDown" ? 1 : -1, items.length));
    } else if (e.key === "Enter") {
      // 入力中の Enter でフォームを送信しない。
      e.preventDefault();
      if (listOpen && idx >= 0) pick(items[idx]);
    } else if (e.key === "Tab") {
      if (listOpen && idx >= 0 && query.trim() !== "" && !e.shiftKey) {
        e.preventDefault();
        pick(items[idx]);
      }
    } else if (e.key === "Escape") {
      if (listOpen || noMatch) {
        e.preventDefault();
        e.stopPropagation();
        setOpen(false);
      }
    } else if (e.key === "Backspace" && query === "" && p.value) {
      clear();
    }
  };

  return (
    <div class="picker skill-picker">
      <div class="skill-input" onClick={() => inputRef.current?.focus()}>
        {p.value && (
          <span class="skill-chip">
            {selected ? krLabel(selected) : p.value}
            <button
              type="button"
              class="ghost"
              aria-label="KR との紐付けを外す"
              disabled={p.disabled}
              onClick={(e) => {
                e.stopPropagation();
                clear();
              }}
            >
              ×
            </button>
          </span>
        )}
        <input
          ref={inputRef}
          role="combobox"
          aria-label={p.ariaLabel}
          aria-expanded={listOpen}
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={listOpen && idx >= 0 ? `${listId}-${idx}` : undefined}
          autocomplete="off"
          value={query}
          placeholder={p.value ? "別の KR に変える" : "KR 名を入力"}
          disabled={p.disabled}
          onInput={(e) => {
            setQuery(e.currentTarget.value);
            setOpen(true);
            setActive(0);
          }}
          onFocus={() => setOpen(true)}
          onBlur={() => {
            setOpen(false);
            setQuery("");
          }}
          onKeyDown={onKeyDown}
        />
      </div>
      {listOpen && (
        <ul class="picker-list" role="listbox" id={listId}>
          {items.map((o, i) => (
            <li
              key={o.id}
              id={`${listId}-${i}`}
              role="option"
              aria-selected={i === idx}
              class={`picker-item${i === idx ? " on" : ""}`}
              // クリックで入力欄がフォーカスを失う前に決定する。
              onMouseDown={(e) => {
                e.preventDefault();
                pick(o);
              }}
              onMouseMove={() => setActive(i)}
            >
              {o.title}
              <span class="muted"> （{o.objective_title}）</span>
            </li>
          ))}
        </ul>
      )}
      {noMatch && (
        <p class="picker-hint">
          {p.options.length === 0
            ? "KR がまだありません。OKR 画面で追加できます。"
            : "該当する KR がありません。OKR 画面で追加できます。"}
        </p>
      )}
    </div>
  );
}

import { useId, useRef, useState } from "preact/hooks";
import type { SkillView } from "./api";
import { filterSkills, normSkill } from "./skill-picker-util";
import { moveIndex } from "./task-picker-util";

interface Props {
  /** 付与済みのスキル名。 */
  value: string[];
  /** 既知のスキル（未取得のうちは空でよい）。 */
  skills: SkillView[];
  onChange: (next: string[]) => void;
  ariaLabel: string;
  disabled?: boolean;
}

/**
 * 既知のスキルだけを選べる入力欄。付いているスキルは外せるチップで並べ、入力で部分一致の候補を出す。
 * キーボード: ↑↓ で移動、Enter / Tab で追加、Backspace（入力が空のとき）で最後のチップを外す、Esc で閉じる。
 * 未知の名前は追加できない（スキル画面で追加する）。
 */
export function SkillPicker(p: Props) {
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listId = useId();

  const items = filterSkills(p.skills, query, p.value);
  const listOpen = open && items.length > 0;
  const idx = items.length === 0 ? -1 : Math.min(active, items.length - 1);
  const noMatch = open && query.trim() !== "" && items.length === 0;
  const already = noMatch && p.value.some((v) => normSkill(v) === normSkill(query));

  const add = (s: SkillView) => {
    p.onChange([...p.value, s.name]);
    setQuery("");
    setActive(0);
    inputRef.current?.focus();
  };
  const removeAt = (i: number) => p.onChange(p.value.filter((_, j) => j !== i));

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
      if (listOpen && idx >= 0) add(items[idx]);
    } else if (e.key === "Tab") {
      // 入力があり候補があるときだけ Tab で追加し、無ければ通常のフォーカス移動。
      if (listOpen && idx >= 0 && query.trim() !== "" && !e.shiftKey) {
        e.preventDefault();
        add(items[idx]);
      }
    } else if (e.key === "Escape") {
      if (listOpen || noMatch) {
        e.preventDefault();
        e.stopPropagation();
        setOpen(false);
      }
    } else if (e.key === "Backspace" && query === "" && p.value.length > 0) {
      removeAt(p.value.length - 1);
    }
  };

  return (
    <div class="picker skill-picker">
      <div class="skill-input" onClick={() => inputRef.current?.focus()}>
        {p.value.map((name, i) => (
          <span class="skill-chip" key={name}>
            {name}
            <button
              type="button"
              class="ghost"
              aria-label={`スキル「${name}」を外す`}
              disabled={p.disabled}
              onClick={(e) => {
                e.stopPropagation();
                removeAt(i);
              }}
            >
              ×
            </button>
          </span>
        ))}
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
          placeholder={p.value.length === 0 ? "スキル名を入力" : ""}
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
          {items.map((s, i) => (
            <li
              key={s.name}
              id={`${listId}-${i}`}
              role="option"
              aria-selected={i === idx}
              class={`picker-item${i === idx ? " on" : ""}`}
              // クリックで入力欄がフォーカスを失う前に決定する。
              onMouseDown={(e) => {
                e.preventDefault();
                add(s);
              }}
              onMouseMove={() => setActive(i)}
            >
              {s.name}
              {s.parent_name && <span class="muted"> （{s.parent_name}）</span>}
            </li>
          ))}
        </ul>
      )}
      {noMatch && (
        <p class="picker-hint">
          {already
            ? "このスキルは付いています。"
            : p.skills.length === 0
              ? "スキルがまだありません。スキル画面で追加できます。"
              : "該当するスキルがありません。スキル画面で追加できます。"}
        </p>
      )}
    </div>
  );
}

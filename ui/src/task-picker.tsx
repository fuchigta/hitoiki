import { useEffect, useId, useRef, useState } from "preact/hooks";
import type { TaskView } from "./api";
import { buildItems, itemLabel, moveIndex, type PickerItem } from "./task-picker-util";

interface Props {
  /** 候補にするタスク（未完了のものだけが使われる）。 */
  tasks: TaskView[];
  /** 選択中のタスク ID（ブロックのフォームで、選んだタスクを入力欄に出す）。 */
  selectedId?: string;
  /** 直前の doing（「戻る」候補）。 */
  back?: TaskView | null;
  ariaLabel: string;
  placeholder?: string;
  autoFocus?: boolean;
  /** 常に候補を開いておく（クイック入力）。Esc は `onEscape` に任せる。 */
  alwaysOpen?: boolean;
  /** Tab で「割り込みとして作成」を切り替える（先頭の `!` でも指定できる）。 */
  interruptToggle?: boolean;
  disabled?: boolean;
  /** 候補が決まった（Enter かクリック）。 */
  onPick: (item: PickerItem) => void;
  /** 選択中のタスクを外す（指定すると × ボタンを出す）。 */
  onClear?: () => void;
  onEscape?: () => void;
  /** 値が変わると入力欄を空にして割り込みのトグルを戻す（ウインドウを開き直したとき）。 */
  resetKey?: number;
}

/**
 * 入力で未完了タスクを絞り込める選択欄（コンボボックス）。一致が無ければ「新規作成」を候補に出す。
 * 候補の組み立ては task-picker-util の純粋関数。キーボード: ↑↓ で移動、Enter で決定、Esc で閉じる。
 * 新規作成そのもの（`taskAdd`）は呼び出し側が `onPick` で行う。
 */
export function TaskPicker(p: Props) {
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);
  const [editing, setEditing] = useState(false);
  const [active, setActive] = useState(0);
  const [interrupt, setInterrupt] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const listId = useId();

  const built = buildItems({
    tasks: p.tasks,
    query,
    interrupt: p.interruptToggle ? interrupt : false,
    back: p.back,
  });
  const items = built.items;
  const listOpen = (p.alwaysOpen || open) && items.length > 0;
  const idx = items.length === 0 ? -1 : Math.min(active, items.length - 1);

  useEffect(() => {
    if (p.autoFocus) inputRef.current?.focus();
  }, [p.autoFocus, p.resetKey]);
  useEffect(() => {
    setQuery("");
    setInterrupt(false);
    setActive(0);
    setOpen(false);
    setEditing(false);
  }, [p.resetKey]);

  const selected = p.selectedId ? p.tasks.find((t) => t.id === p.selectedId) : undefined;
  const shown = editing || !selected ? query : selected.title;

  const pick = (it: PickerItem) => {
    setQuery("");
    setEditing(false);
    setOpen(false);
    setActive(0);
    p.onPick(it);
  };

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.isComposing) return; // IME の変換確定中は触らない
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!listOpen && !p.alwaysOpen) {
        setOpen(true);
        setActive(0);
        return;
      }
      setActive(moveIndex(idx, e.key === "ArrowDown" ? 1 : -1, items.length));
    } else if (e.key === "Enter") {
      if (listOpen && idx >= 0) {
        e.preventDefault();
        pick(items[idx]);
      } else if (p.alwaysOpen) {
        e.preventDefault();
      }
    } else if (e.key === "Escape") {
      if (p.alwaysOpen) {
        e.preventDefault();
        p.onEscape?.();
      } else if (listOpen) {
        e.preventDefault();
        e.stopPropagation();
        setOpen(false);
      } else {
        p.onEscape?.();
      }
    } else if (e.key === "Tab" && p.interruptToggle && !e.shiftKey) {
      e.preventDefault();
      setInterrupt((v) => !v);
    }
  };

  return (
    <div class="picker">
      <div class="picker-input">
        <input
          ref={inputRef}
          role="combobox"
          aria-label={p.ariaLabel}
          aria-expanded={listOpen}
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={listOpen && idx >= 0 ? `${listId}-${idx}` : undefined}
          autocomplete="off"
          value={shown}
          placeholder={p.placeholder}
          disabled={p.disabled}
          onInput={(e) => {
            setQuery(e.currentTarget.value);
            setEditing(true);
            setOpen(true);
            setActive(0);
          }}
          onFocus={(e) => {
            if (selected && !editing) e.currentTarget.select();
          }}
          onBlur={() => {
            if (p.alwaysOpen) return;
            setOpen(false);
            setEditing(false);
            setQuery("");
          }}
          onClick={() => !p.alwaysOpen && setOpen(true)}
          onKeyDown={onKeyDown}
        />
        {selected && p.onClear && (
          <button type="button" class="ghost" aria-label="タスクを外す" onClick={p.onClear} disabled={p.disabled}>
            ×
          </button>
        )}
      </div>
      {p.interruptToggle && (
        <p class="picker-hint">
          <span class={`picker-flag${built.interrupt ? " on" : ""}`}>割り込み: {built.interrupt ? "オン" : "オフ"}</span>
          Tab で切り替え（先頭に ! でも指定）。新規作成のときに使います。
        </p>
      )}
      {listOpen && (
        <ul class="picker-list" role="listbox" id={listId}>
          {items.map((it, i) => (
            <li
              key={`${it.kind}:${it.kind === "create" ? it.title : it.task.id}`}
              id={`${listId}-${i}`}
              role="option"
              aria-selected={i === idx}
              class={`picker-item ${it.kind}${i === idx ? " on" : ""}`}
              // クリックで入力欄がフォーカスを失う前に決定する。
              onMouseDown={(e) => {
                e.preventDefault();
                pick(it);
              }}
              onMouseMove={() => setActive(i)}
            >
              {itemLabel(it)}
              {it.kind === "task" && it.task.status === "doing" && <span class="meta"> （進行中）</span>}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

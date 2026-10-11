import { useEffect, useState } from "preact/hooks";
import { type DeleteOutcome } from "./api";
import { ErrorBox } from "./components";
import { useAction } from "./lib";
import { hasImpact, impactLines } from "./manage-util";

export interface DeleteChoice {
  label: string;
  initial: string;
  options: { value: string; label: string }[];
}

interface Props {
  /** ボタンの文言。 */
  label?: string;
  /** 何を消すか（ダイアログの見出しに入れる）。例: 「KR」。 */
  what: string;
  /** dry_run で影響を数える。`choice` があれば選択値を渡す。 */
  preview: (choice: string) => Promise<DeleteOutcome>;
  /** 確認後に削除する（force つき）。 */
  commit: (choice: string) => Promise<unknown>;
  onDeleted: () => void;
  /** 削除の扱いの選択肢（スキルの子の扱いなど）。 */
  choice?: DeleteChoice;
  disabled?: boolean;
}

/** 削除ボタン。押すと影響を確認するダイアログ（アプリ内。`window.confirm` は使わない）を出し、了承されたら削除する。 */
export function ConfirmDelete({ label = "削除", what, preview, commit, onDeleted, choice, disabled }: Props) {
  const [open, setOpen] = useState<DeleteOutcome | null>(null);
  const [pick, setPick] = useState(choice?.initial ?? "");
  const act = useAction();

  const load = async (c: string) => {
    const r = await act.run(() => preview(c));
    if (r) setOpen(r);
  };

  useEffect(() => {
    if (!open) return;
    const h = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(null);
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [open]);

  return (
    <>
      <button
        type="button"
        class="ghost"
        disabled={disabled || act.busy}
        onClick={() => {
          setPick(choice?.initial ?? "");
          void load(choice?.initial ?? "");
        }}
      >
        {label}
      </button>
      {!open && <ErrorBox error={act.error} />}
      {open && (
        <div class="modal-back">
          <div class="modal" role="alertdialog" aria-modal="true" aria-labelledby="del-title">
            <p id="del-title">
              {what}「{open.impact.name}」を削除しますか？
            </p>
            {choice && (
              <label class="field">
                <span>{choice.label}</span>
                <select
                  value={pick}
                  onChange={(e) => {
                    setPick(e.currentTarget.value);
                    void load(e.currentTarget.value);
                  }}
                >
                  {choice.options.map((o) => (
                    <option key={o.value} value={o.value}>
                      {o.label}
                    </option>
                  ))}
                </select>
              </label>
            )}
            {hasImpact(open.impact) ? (
              <>
                <p class="muted">次のものに影響します。</p>
                <ul class="plain">
                  {impactLines(open.impact).map((l) => (
                    <li key={l}>{l}</li>
                  ))}
                </ul>
              </>
            ) : (
              <p class="muted">ほかのデータへの影響はありません。</p>
            )}
            {open.impact.note && <p class="muted">{open.impact.note}</p>}
            <ErrorBox error={act.error} />
            <div class="row">
              <span class="spacer" />
              <button autoFocus type="button" disabled={act.busy} onClick={() => setOpen(null)}>
                キャンセル
              </button>
              <button
                type="button"
                class="primary"
                disabled={act.busy}
                onClick={async () => {
                  const r = await act.run(() => commit(pick));
                  if (r !== undefined) {
                    setOpen(null);
                    onDeleted();
                  }
                }}
              >
                削除する
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}

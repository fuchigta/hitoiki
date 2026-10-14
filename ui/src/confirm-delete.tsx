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

interface HookProps {
  /** 何を消すか（ダイアログの見出しに入れる）。例: 「成果の指標」。 */
  what: string;
  /** dry_run で影響を数える。`choice` があれば選択値を渡す。 */
  preview: (choice: string) => Promise<DeleteOutcome>;
  /** 確認後に削除する（force つき）。 */
  commit: (choice: string) => Promise<unknown>;
  onDeleted: () => void;
  /** 削除の扱いの選択肢（スキルの子の扱いなど）。 */
  choice?: DeleteChoice;
}

interface Props extends HookProps {
  /** ボタンの文言。 */
  label?: string;
  disabled?: boolean;
}

/**
 * 削除の確認ダイアログ（アプリ内。`window.confirm` は使わない）を出す部品の本体。
 * `start()` で影響を数えて確認を開き、了承されたら削除する。ボタンの代わりに「…」メニューの項目から
 * 呼びたいときに使う（`dialog` を行の中のどこかに置く）。
 */
export function useConfirmDelete({ what, preview, commit, onDeleted, choice }: HookProps) {
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

  const start = () => {
    setPick(choice?.initial ?? "");
    void load(choice?.initial ?? "");
  };

  const dialog = (
    <>
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
                <p class="hint">次のものに影響します。</p>
                <ul class="plain">
                  {impactLines(open.impact).map((l) => (
                    <li key={l}>{l}</li>
                  ))}
                </ul>
              </>
            ) : (
              <p class="hint">ほかのデータへの影響はありません。</p>
            )}
            {open.impact.note && <p class="hint">{open.impact.note}</p>}
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
  return { start, busy: act.busy, dialog };
}

/** 削除ボタン。押すと影響を確認するダイアログを出し、了承されたら削除する。 */
export function ConfirmDelete({ label = "削除", disabled, ...rest }: Props) {
  const d = useConfirmDelete(rest);
  return (
    <>
      <button type="button" class="ghost" disabled={disabled || d.busy} onClick={d.start}>
        {label}
      </button>
      {d.dialog}
    </>
  );
}

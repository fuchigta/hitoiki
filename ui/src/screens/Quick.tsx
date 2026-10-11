import { useEffect, useState } from "preact/hooks";
import { api, on, type TaskView } from "../api";
import { ErrorBox } from "../components";
import { useAction } from "../lib";
import { TaskPicker } from "../task-picker";
import type { PickerItem } from "../task-picker-util";

/** 開始後に結果を見せてから閉じるまでの時間。 */
const SHOW_MS = 700;

/**
 * クイック入力ウインドウ（ウインドウラベル `quick`）。グローバルショートカットで開く。
 * 入力 → Enter で、既存タスクの開始・新規作成して開始・直前のタスクに戻る。Esc かフォーカスを失うと閉じる。
 */
export function Quick() {
  const [tasks, setTasks] = useState<TaskView[]>([]);
  const [back, setBack] = useState<TaskView | null>(null);
  const [resetKey, setResetKey] = useState(0);
  const [done, setDone] = useState<string | null>(null);
  const act = useAction();

  const load = () => {
    api.taskList(["todo", "doing"]).then((l) => setTasks(l.tasks)).catch(() => {});
    api.quickPrevious().then(setBack).catch(() => setBack(null));
  };
  const open = () => {
    setDone(null);
    act.clear();
    setResetKey((k) => k + 1);
    load();
  };

  useEffect(() => {
    open();
    const off = on("quick-open", open).catch(() => () => {});
    return () => {
      off.then((f) => f());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const close = () => api.quickClose().catch(() => {});

  const pick = async (it: PickerItem) => {
    const r = await act.run(() => {
      if (it.kind === "back") return api.quickBack();
      if (it.kind === "create") return api.quickSwitch({ query: it.title, new: true, interrupt: it.interrupt });
      return api.quickSwitch({ query: it.task.id });
    });
    if (r) {
      setDone(`開始しました: ${r.task.title}`);
      window.setTimeout(close, SHOW_MS);
    }
  };

  if (done) {
    return (
      <div class="quick">
        <p class="quick-done" role="status">
          {done}
        </p>
      </div>
    );
  }
  return (
    <div class="quick">
      <TaskPicker
        tasks={tasks}
        back={back}
        ariaLabel="タスクをすばやく入力"
        placeholder="タスク名を入力して Enter（開始します）"
        autoFocus
        alwaysOpen
        interruptToggle
        disabled={act.busy}
        resetKey={resetKey}
        onPick={pick}
        onEscape={close}
      />
      <ErrorBox error={act.error} />
    </div>
  );
}

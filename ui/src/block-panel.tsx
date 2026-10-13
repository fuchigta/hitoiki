import { api } from "./api";
import type { BlockInput, BlockView, CalendarEvent, Millis, TaskView } from "./api";
import { BlockForm, blockTitle, type BlockDraft } from "./block-form";
import { Section } from "./components";
import { fmtTime, useAction } from "./lib";
import { OverflowMenu } from "./ui/parts";

interface Props {
  draft: BlockDraft;
  /** 日付（YYYY-MM-DD）。 */
  date: string;
  dayEnd: Millis;
  tasks: TaskView[];
  blocks: BlockView[];
  events: CalendarEvent[];
  /** 保存・削除が済んだ（一覧の再取得はここで行う）。 */
  onChanged: () => void;
  onClose: () => void;
}

/** サイドパネルのブロックの作成・編集。削除は見出しの「…」メニューから。 */
export function BlockPanel(p: Props) {
  const act = useAction();
  const d = p.draft;

  const others = [
    ...p.blocks.map((b) => ({ id: b.id, start_at: b.start_at, end_at: b.end_at, title: blockTitle(b) })),
    ...p.events
      .filter((c) => c.is_busy && !c.all_day)
      .map((c) => ({ start_at: c.start_at, end_at: c.end_at, title: c.title })),
  ];

  const submit = async (input: BlockInput, id?: string) => {
    const r = await act.run(() => (id ? api.blockUpdate(id, input) : api.blockAdd(input)));
    if (r) {
      p.onChanged();
      p.onClose();
    }
  };

  const remove = async () => {
    if (!d.id) return;
    const id = d.id;
    const r = await act.run(() => api.blockDelete(id));
    if (r !== undefined) {
      p.onChanged();
      p.onClose();
    }
  };

  return (
    <Section
      title={d.id ? "ブロックを編集" : "ブロックを追加"}
      aside={
        d.id ? (
          <OverflowMenu label="ブロックの操作" items={[{ label: "削除", onSelect: remove, disabled: act.busy }]} />
        ) : undefined
      }
    >
      <p class="meta">
        {fmtTime(d.start)}–{fmtTime(d.end)}
      </p>
      <BlockForm
        key={`${d.id ?? "new"}-${d.start}-${d.end}`}
        draft={d}
        date={p.date}
        dayEnd={p.dayEnd}
        tasks={p.tasks}
        others={others}
        busy={act.busy}
        error={act.error}
        onSubmit={submit}
        onCancel={p.onClose}
      />
    </Section>
  );
}

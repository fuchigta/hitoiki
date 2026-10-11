import { api } from "../api";
import { Bars, Card, Empty, ErrorBox, Timeline } from "../components";
import { showStartGuide } from "../onboarding-util";
import { KIND_LABEL, fmtDur, fmtTime, useAction, useAsync, useLiveTick } from "../lib";

export function Today() {
  const tick = useLiveTick();
  const st = useAsync(() => api.status(), [tick]);
  const log = useAsync(() => api.log({ date: "today" }), [tick]);
  const blocks = useAsync(() => api.blockList({ date: "today" }), [tick]);
  const act = useAction();

  const s = st.data;
  const nothing = !st.loading && !s;

  return (
    <div class="stack">
      <ErrorBox error={st.error ?? log.error ?? blocks.error ?? act.error} />
      {nothing && <Empty>状況を取得できませんでした。</Empty>}
      {s && (
        <div class="grid3">
          <Card title="いま">
            {s.doing.length === 0 ? (
              <Empty>進行中のタスクはありません。</Empty>
            ) : (
              <ul class="plain">
                {s.doing.map((d) => (
                  <li key={d.task.id}>
                    <strong>{d.task.title}</strong>
                    {d.since && (
                      <div class="muted">
                        {fmtTime(d.since)} から（{fmtDur(s.now - d.since)}）
                      </div>
                    )}
                    <div class="row">
                      <button disabled={act.busy} onClick={() => act.run(() => api.taskDone(d.task.id)).then(st.reload)}>
                        完了にする
                      </button>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </Card>
          <Card title="今日の合計">
            <p class="big">{fmtDur(s.today.total_ms)}</p>
            <p class="muted">
              無操作 {fmtDur(s.today.idle_ms)} ・ 休憩 {fmtDur(s.today.break_ms)}（合計には含めません）
            </p>
          </Card>
          <Card title="確認待ち">
            <p class="big">{s.needs_review_count}件</p>
            <p class="muted">
              {s.needs_review_count > 0 ? (
                <a href="#/review">仕分けを開く</a>
              ) : (
                "仕分けが必要な区間はありません。"
              )}
              {s.needs_review_short_count > 0 && ` 短い区間 ${s.needs_review_short_count} 件は仕分けで表示できます。`}
            </p>
          </Card>
        </div>
      )}

      {s && log.data && showStartGuide(s.doing.length, log.data.segments.length) && (
        <Card title="次にやること">
          <ul>
            {s.doing.length === 0 && (
              <li>
                <a href="#/tasks">タスク</a>を 1 つ作って開始すると、ルールに当たらない時間の候補になります。
              </li>
            )}
            {log.data.segments.length === 0 && (
              <li>記録はもう始まっています。しばらく使うと、ここにタイムラインが並びます。</li>
            )}
            <li>
              使い方は<a href="#/onboarding">はじめに</a>でいつでも見られます。
            </li>
          </ul>
        </Card>
      )}

      <Card title="タイムライン">
        {s && log.data && blocks.data ? (
          log.data.segments.length === 0 && blocks.data.blocks.length === 0 ? (
            <Empty>まだ記録がありません。</Empty>
          ) : (
            <Timeline
              segments={log.data.segments}
              blocks={blocks.data.blocks}
              gaps={log.data.gaps}
              now={s.now}
              dayStart={s.today.range.start_at}
              dayEnd={s.today.range.end_at}
            />
          )
        ) : (
          <Empty>読み込み中…</Empty>
        )}
      </Card>

      {s && (
        <div class="grid2">
          <Card title="種別ごと">
            <Bars rows={s.today.by_kind.map((r) => ({ ...r, label: KIND_LABEL[r.key as keyof typeof KIND_LABEL] ?? r.label }))} total={s.today.total_ms} />
          </Card>
          <Card title="タスクごと（上位）">
            <Bars rows={s.today.top_tasks} total={s.today.total_ms} />
          </Card>
        </div>
      )}

      {s && s.unanswered_pushes.length > 0 && (
        <Card title="未回答の問いかけ">
          <ul class="plain">
            {s.unanswered_pushes.map((p) => (
              <li key={p.id}>
                {fmtTime(p.at)} <span class="muted">{p.trigger}</span>
              </li>
            ))}
          </ul>
          <p class="muted">答えなくても大丈夫です。</p>
        </Card>
      )}
    </div>
  );
}

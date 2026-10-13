// 期間の選択肢を 1 か所で定義する（core の `today` / `yesterday` / `this-week` / `last-week` と対応）。

export type PeriodId = "today" | "yesterday" | "this-week" | "last-week";

export const PERIOD_LABELS: Record<PeriodId, string> = {
  today: "今日",
  yesterday: "昨日",
  "this-week": "今週",
  "last-week": "先週",
};

export const ALL_PERIODS: PeriodId[] = ["today", "yesterday", "this-week", "last-week"];

/** 画面で使える選択肢（`only` の順に並べる。省略時は全部）。 */
export function periodOptions(only?: PeriodId[]): { id: PeriodId; label: string }[] {
  return (only ?? ALL_PERIODS).map((id) => ({ id, label: PERIOD_LABELS[id] }));
}

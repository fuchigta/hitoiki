import { dateFromParam, shiftDate, weekdayLabel } from "./today-util";

interface Props {
  /** 表示している日（YYYY-MM-DD）。 */
  date: string;
  today: string;
  isToday: boolean;
  /** 日付を移す。 */
  onGo: (date: string) => void;
  /** 「デザイン」のチップを点灯させる（デザインのパネルが出ているか、デザイン中）。 */
  designOn: boolean;
  /** デザイン中（「完了」まで保つモード）。 */
  designMode: boolean;
  onToggleDesign: () => void;
  showAllDay: boolean;
  onShowAllDay: (on: boolean) => void;
}

/** 今日の画面の上の帯。日付の移動、デザインの切り替え、0〜24 時の表示。 */
export function TodayDateStrip(p: Props) {
  return (
    <div class="date-strip" role="group" aria-label="日付">
      <button type="button" aria-label="前の日" onClick={() => p.onGo(shiftDate(p.date, -1))}>
        ‹
      </button>
      <button type="button" disabled={p.isToday} onClick={() => p.onGo(p.today)}>
        今日
      </button>
      <button type="button" aria-label="次の日" onClick={() => p.onGo(shiftDate(p.date, 1))}>
        ›
      </button>
      <input
        type="date"
        value={p.date}
        aria-label="日付"
        onChange={(e) => {
          const v = e.currentTarget.value;
          if (v) p.onGo(dateFromParam(v, p.today));
        }}
      />
      <span class="meta">{weekdayLabel(p.date)}</span>
      <span class="spacer" />
      <button type="button" class={p.designOn ? "chip on" : "chip"} aria-pressed={p.designMode} onClick={p.onToggleDesign}>
        デザイン
      </button>
      <label class="check">
        <input type="checkbox" checked={p.showAllDay} onChange={(e) => p.onShowAllDay(e.currentTarget.checked)} />
        0〜24 時を表示
      </label>
    </div>
  );
}

// 今日の画面のサイドパネルの状態（同時に 1 つ）と、デザインのパネルを自動で開く条件。
import { useEffect, useReducer, useRef } from "preact/hooks";
import type { BlockList } from "./api";
import { shouldDesign } from "./plan-util";
import { SIDE_INITIAL, sideReducer, visiblePanel, type SideAction, type SidePanel, type SideState } from "./today-util";

const DONE_KEY = "hitoiki.design.done";

/** 今日のデザインを済ませた日（YYYY-MM-DD）。保存できない環境では常に未完了。 */
export function designDoneDate(): string | null {
  try {
    return localStorage.getItem(DONE_KEY);
  } catch {
    return null;
  }
}

/** 今日のデザインを済ませた日を覚える（保存できなくても動く）。 */
export function markDesignDone(date: string) {
  try {
    localStorage.setItem(DONE_KEY, date);
  } catch {
    /* 保存できなくても動く */
  }
}

interface Options {
  /** `#/today/design` で開かれた（通知・振り返りから）。 */
  designRequested: boolean;
  isToday: boolean;
  /** 表示している日（YYYY-MM-DD）。 */
  date: string;
  /** その日のブロック（届くまで undefined）。 */
  blocks: BlockList | undefined;
}

export interface TodaySide {
  side: SideState;
  /** 実際に出すパネル。 */
  panel: SidePanel | null;
  dispatch: (a: SideAction) => void;
}

/** サイドパネルの状態。今日のデザインは「完了」まで保つ。 */
export function useTodaySide(o: Options): TodaySide {
  const [side, dispatch] = useReducer(sideReducer, SIDE_INITIAL);
  const panel = visiblePanel(side);

  // 通知・振り返りから `#/today/design` で開かれたら、デザインのパネルを出す。
  useEffect(() => {
    if (o.designRequested) dispatch({ type: "open", panel: { kind: "design" } });
  }, [o.designRequested]);

  // 今日のブロックが 0 件で、まだデザインを済ませていなければ、最初に 1 度だけ自動で開く。
  const autoChecked = useRef(false);
  const blocksLoaded = !!o.blocks;
  const blockCount = o.blocks?.blocks.length ?? 0;
  useEffect(() => {
    if (autoChecked.current || !blocksLoaded) return;
    autoChecked.current = true;
    if (o.isToday && shouldDesign(blockCount, false, designDoneDate() === o.date)) dispatch({ type: "open", panel: { kind: "design" } });
  }, [blocksLoaded]);

  return { side, panel, dispatch };
}

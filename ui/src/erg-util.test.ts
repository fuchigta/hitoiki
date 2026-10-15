import { describe, expect, it } from "vitest";
import type { ErgDistribution, ErgLabel } from "./api/types";
import {
  classifyMessage,
  distributionRows,
  distributionSummary,
  ergKey,
  ergState,
  fmtConfidence,
  labelMap,
  weekOf,
} from "./erg-util";

const label = (p: Partial<ErgLabel>): ErgLabel => ({
  record_kind: "concern",
  record_id: "H1",
  tag: "growth",
  source: "ai",
  confidence: 0.9,
  reason: "学び",
  at: 1,
  ...p,
});

const dist = (p: Partial<ErgDistribution> = {}): ErgDistribution => ({
  existence: 0,
  relatedness: 0,
  growth: 0,
  none: 0,
  unconfirmed: 0,
  unlabeled: 0,
  ai_decided: 0,
  total: 0,
  legend: "",
  ...p,
});

describe("ラベルの状態", () => {
  it("出所から画面での状態を決める", () => {
    expect(ergState(undefined)).toBe("unlabeled");
    expect(ergState(null)).toBe("unlabeled");
    expect(ergState(label({ source: "user" }))).toBe("user");
    expect(ergState(label({ source: "ai" }))).toBe("ai");
    expect(ergState(label({ source: "unconfirmed" }))).toBe("unconfirmed");
  });

  it("記録の種類と ID で引き当てる", () => {
    const m = labelMap([label({}), label({ record_kind: "reframe", record_id: "A1", tag: "none" })]);
    expect(m.get(ergKey("concern", "H1"))?.tag).toBe("growth");
    expect(m.get(ergKey("reframe", "A1"))?.tag).toBe("none");
    expect(m.get(ergKey("condition", "H1"))).toBeUndefined();
  });

  it("確信度は百分率で表示し、範囲外は丸める", () => {
    expect(fmtConfidence(0.856)).toBe("86%");
    expect(fmtConfidence(1.4)).toBe("100%");
    expect(fmtConfidence(null)).toBe("");
    expect(fmtConfidence(Number.NaN)).toBe("");
  });
});

describe("分布の表示", () => {
  it("0 件の観点も含めて 4 行出す", () => {
    const rows = distributionRows(dist({ growth: 3, total: 3 }));
    expect(rows.map((r) => [r.key, r.count])).toEqual([
      ["existence", 0],
      ["relatedness", 0],
      ["growth", 3],
      ["none", 0],
    ]);
    expect(rows[0].hint).toBe("体や生活の安心");
  });

  it("要約は件数を述べるだけで、評価の言葉を含まない", () => {
    expect(distributionSummary(dist())).toBe("この期間に、文章のある記録はありません。");
    const s = distributionSummary(dist({ existence: 1, relatedness: 2, growth: 3, none: 1, unconfirmed: 2, unlabeled: 1, total: 10 }));
    expect(s).toBe("記録 10 件のうち、存在 1 ・ 関係 2 ・ 成長 3 ・ どれにも当てはまらない 1（未確定 2、未判定 1）");
    for (const ng of ["少ない", "多すぎ", "べき", "偏り", "足りない", "不足"]) expect(s).not.toContain(ng);
  });
});

describe("判定結果のメッセージ", () => {
  it("AI 未設定は理由をそのまま出す", () => {
    expect(classifyMessage({ skipped: "AI が設定されていないため判定しません", targets: 0, adopted: 0, unconfirmed: 0, unread: 0 })).toBe(
      "AI が設定されていないため判定しません",
    );
  });
  it("対象なし・結果あり・判定が得られなかったもの", () => {
    expect(classifyMessage({ skipped: null, targets: 0, adopted: 0, unconfirmed: 0, unread: 0 })).toBe("判定する記録はありません。");
    expect(classifyMessage({ skipped: null, targets: 3, adopted: 2, unconfirmed: 1, unread: 0 })).toBe(
      "3 件を判定しました。AI の判定を採用 2 件、未確定 1 件。",
    );
    expect(classifyMessage({ skipped: null, targets: 3, adopted: 1, unconfirmed: 1, unread: 1 })).toContain("判定が得られなかったもの 1 件");
  });
});

describe("週の範囲", () => {
  it("月曜始まりで、その日を含む週を返す", () => {
    expect(weekOf("2026-10-10")).toEqual({ from: "2026-10-05", to: "2026-10-11" }); // 土曜
    expect(weekOf("2026-10-05")).toEqual({ from: "2026-10-05", to: "2026-10-11" }); // 月曜
    expect(weekOf("2026-10-11")).toEqual({ from: "2026-10-05", to: "2026-10-11" }); // 日曜
  });
  it("月またぎ・年またぎ", () => {
    expect(weekOf("2026-01-01")).toEqual({ from: "2025-12-29", to: "2026-01-04" });
  });
  it("不正な日付は null", () => {
    expect(weekOf("")).toBeNull();
    expect(weekOf("2026/10/10")).toBeNull();
  });
});

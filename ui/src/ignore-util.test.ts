import { describe, expect, it } from "vitest";
import type { CorrectionCandidate, IgnoreSuggestion } from "./api";
import { fmtTotal, hasExe, ignorableIds, normalizeExe, suggestionLine } from "./ignore-util";

const cand = (id: string, checked: boolean, gap = false) => ({ id, checked, gap }) as CorrectionCandidate;

describe("ignore-util", () => {
  it("normalizeExe はパスと空白を除く", () => {
    expect(normalizeExe("  C:\\Windows\\explorer.exe ")).toBe("explorer.exe");
    expect(normalizeExe("/usr/bin/x.exe")).toBe("x.exe");
    expect(normalizeExe("  ")).toBe("");
  });
  it("hasExe は大文字小文字を区別しない", () => {
    expect(hasExe(["Explorer.exe"], "explorer.EXE")).toBe(true);
    expect(hasExe(["a.exe"], "b.exe")).toBe(false);
  });
  it("ignorableIds はチェック済みで記録なし以外", () => {
    expect(ignorableIds([cand("a", true), cand("b", false), cand("gap:1", true, true)])).toEqual(["a"]);
  });
  it("fmtTotal", () => {
    expect(fmtTotal(30_000)).toBe("30秒");
    expect(fmtTotal(990_000)).toBe("16.5分");
  });
  it("suggestionLine", () => {
    const sg = { exe: "explorer.exe", short_count: 214, span_count: 229, total_ms: 990_000 } as IgnoreSuggestion;
    expect(suggestionLine(sg)).toBe("explorer.exe は短い区間が 214 件（合計 16.5分）。");
    expect(suggestionLine({ ...sg, short_count: 0 })).toBe("explorer.exe は区間が 229 件（合計 16.5分）。");
  });
});

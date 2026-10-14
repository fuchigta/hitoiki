import { describe, expect, it } from "vitest";
import type { KrCandidate, KrSuggestions, KrSummary } from "./api";
import { aiNote, candidateRef, kindLabel, sourceLabel, without, wantsSuggestion } from "./kr-suggest-util";

const cand = (kind: KrCandidate["kind"], id: string, source: KrCandidate["source"] = "local"): KrCandidate => ({
  kind,
  id,
  name: id,
  score: 0.5,
  source,
});
const sugg = (ai: KrSuggestions["ai"]): KrSuggestions => ({
  key_result: { id: "k", title: "K" },
  eligible: true,
  candidates: [],
  ai,
});

describe("candidateRef", () => {
  it("採否のコマンドに渡す指定になる", () => {
    expect(candidateRef(cand("task", "T1"))).toBe("task:T1");
    expect(candidateRef(cand("skill", "S1"))).toBe("skill:S1");
  });
});

describe("ラベル", () => {
  it("種類と出どころの日本語", () => {
    expect(kindLabel("task")).toBe("タスク");
    expect(kindLabel("skill")).toBe("スキル");
    expect(sourceLabel("local")).toBe("名前の近さ");
    expect(sourceLabel("ai")).toBe("AI");
    expect(sourceLabel("both")).toBe("名前の近さと AI");
  });
});

describe("wantsSuggestion", () => {
  const sum = (kr_skills: string[], statuses: string[]) =>
    ({ kr_skills, tasks: statuses.map((status) => ({ status })) }) as unknown as KrSummary;
  it("スキルも紐付けタスクも無い成果の指標だけ", () => {
    expect(wantsSuggestion(null)).toBe(false);
    expect(wantsSuggestion(sum([], []))).toBe(true);
    expect(wantsSuggestion(sum([], ["dropped"]))).toBe(true);
    expect(wantsSuggestion(sum(["Rust"], []))).toBe(false);
    expect(wantsSuggestion(sum([], ["todo"]))).toBe(false);
  });
});

describe("aiNote", () => {
  it("聞いたか・使えたかで一言が変わる", () => {
    expect(aiNote(sugg({ used: false, note: "失敗" }), true)).toBe("失敗");
    expect(aiNote(sugg({ used: false, note: null }), true)).toContain("設定されていない");
    expect(aiNote(sugg({ used: false, note: null }), false)).toBeNull();
    expect(aiNote(sugg({ used: true, note: null }), true)).toContain("合わせて");
  });
});

describe("without", () => {
  it("種類と ID が一致する候補だけを取り除く", () => {
    const l = [cand("task", "A"), cand("skill", "A"), cand("task", "B")];
    expect(without(l, { kind: "task", id: "A" }).map(candidateRef)).toEqual(["skill:A", "task:B"]);
  });
});

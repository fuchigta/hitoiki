import { describe, expect, it } from "vitest";
import type { Question, StructuredOutput } from "./api/types";
import {
  askedScales,
  buildAnswers,
  detailLines,
  hasAnswers,
  initialForm,
  initialForms,
  isBlank,
  parseAnswered,
  parseStructured,
} from "./reflect-util";

const out: StructuredOutput = {
  summary: "要約",
  questions: [
    { kind: "checkin", prompt: "調子は", scales: ["energy", "mood"] },
    { kind: "hassle", prompt: "引っかかったことは" },
    { kind: "abc", prompt: "場面を選んで", activating_event: "指摘された", belief_prompt: "何がよぎった" },
    { kind: "free", prompt: "ひとことで" },
  ],
};

describe("parseStructured / parseAnswered", () => {
  it("構造化されていない・壊れている場合は null / 空", () => {
    expect(parseStructured({ structured_json: null })).toBeNull();
    expect(parseStructured({ structured_json: "{" })).toBeNull();
    expect(parseStructured({ structured_json: '{"questions":[]}' })).toBeNull();
    expect(parseAnswered({ answers_json: null })).toEqual({ items: [] });
    expect(parseAnswered({ answers_json: "x" })).toEqual({ items: [] });
  });
  it("読める", () => {
    expect(parseStructured({ structured_json: JSON.stringify(out) })?.questions).toHaveLength(4);
    const a = parseAnswered({ answers_json: '{"items":[{"index":1,"kind":"hassle","record_id":"H1"}]}' });
    expect(a.items[0].record_id).toBe("H1");
  });
});

describe("initialForm", () => {
  it("ハッスルの強度は 3、ABC の A は候補", () => {
    const f = initialForms(out);
    expect(f[1].intensity).toBe(3);
    expect(f[2].activating_event).toBe("指摘された");
    expect(f[0].intensity).toBeUndefined();
  });
});

describe("buildAnswers", () => {
  it("何も入力していなければ空（初期状態でも送らない）", () => {
    const forms = initialForms(out);
    expect(buildAnswers(out.questions, forms)).toEqual({ answers: [] });
    expect(hasAnswers(out.questions, forms, new Set())).toBe(false);
  });

  it("種類ごとに回答 JSON を作る", () => {
    const forms = initialForms(out);
    forms[0] = { ...forms[0], energy: 3, mood: 4, note: " 普通 " };
    forms[1] = { ...forms[1], text: " 会議が長い ", intensity: 4 };
    forms[2] = { ...forms[2], belief: "向いていない", emotion: "落ち込み", emotion_intensity: 4, dispute: "" };
    forms[3] = { ...forms[3], text: "まあまあ" };
    const a = buildAnswers(out.questions, forms);
    expect(a.answers).toHaveLength(4);
    expect(JSON.parse(JSON.stringify(a))).toEqual({
      answers: [
        { index: 0, energy: 3, mood: 4, note: "普通" },
        { index: 1, text: "会議が長い", intensity: 4 },
        {
          index: 2,
          activating_event: "指摘された",
          belief: "向いていない",
          emotion: "落ち込み",
          emotion_intensity: 4,
        },
        { index: 3, text: "まあまあ" },
      ],
    });
  });

  it("尋ねていない尺度は送らない", () => {
    const forms = initialForms(out);
    forms[0] = { ...forms[0], energy: 2, stress: 5 };
    const a = buildAnswers(out.questions, forms).answers[0];
    expect(a.energy).toBe(2);
    expect(a.stress).toBeUndefined();
  });

  it("回答済みの問いと空の問いは含めない", () => {
    const forms = initialForms(out);
    forms[0] = { ...forms[0], energy: 3 };
    forms[3] = { ...forms[3], text: "x" };
    const a = buildAnswers(out.questions, forms, new Set([0]));
    expect(a.answers.map((x) => x.index)).toEqual([3]);
  });

  it("ABC は A を編集しただけでも送る。候補のままで他が空なら送らない", () => {
    const q = out.questions[2];
    const f = initialForm(q);
    expect(isBlank(q, f)).toBe(true);
    expect(isBlank(q, { ...f, activating_event: "別の出来事" })).toBe(false);
    expect(isBlank(q, { ...f, activating_event: "", belief: "b" })).toBe(false);
  });
});

describe("askedScales", () => {
  it("空なら 3 つとも", () => {
    const q: Question = { kind: "checkin", prompt: "x" };
    expect(askedScales(q)).toEqual(["energy", "mood", "stress"]);
    expect(askedScales({ ...q, scales: ["mood"] })).toEqual(["mood"]);
  });
});

describe("detailLines", () => {
  const base = { id: "x", at: 0, push_id: null };
  it("保存したチェックイン・ハッスル・ABC の内容を行にする", () => {
    expect(
      detailLines({
        index: 0,
        kind: "checkin",
        prompt: "p",
        checkin: { ...base, energy: 3, mood: null, stress: 2, erg_tag: "growth", note: "普通" },
      }),
    ).toEqual(["元気: 3", "ストレス: 2", "ERG タグ: 成長", "メモ: 普通"]);
    expect(
      detailLines({
        index: 1,
        kind: "hassle",
        prompt: "p",
        hassle: { ...base, text: "会議が長い", intensity: 4, segment_id: null },
      }),
    ).toEqual(["気になったこと: 会議が長い", "強さ: 4"]);
    expect(
      detailLines({
        index: 2,
        kind: "abc",
        prompt: "p",
        abc: {
          id: "x",
          at: 0,
          activating_event: "指摘された",
          belief: "向いていない",
          consequence_emotion: "落ち込み",
          consequence_intensity: 4,
          dispute: "一部だけ",
          ai_dialog_md: null,
          hassle_id: null,
        },
      }),
    ).toEqual([
      "A: 出来事: 指摘された",
      "B: 受け止め方: 向いていない",
      "C: 感情: 落ち込み",
      "感情の強さ: 4",
      "別の捉え方・反論: 一部だけ",
    ]);
    expect(detailLines({ index: 3, kind: "free", prompt: "p", text: "x" })).toEqual([]);
  });
});

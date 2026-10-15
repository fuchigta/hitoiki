// 振り返りの構造化された問いに答えるフォームの、状態と回答 JSON の相互変換（純粋関数）。

import { ERG_NAMES } from "./erg-util";
import type {
  AnswerDetail,
  AnswerItem,
  AnswersInput,
  AnswersRecord,
  Question,
  Reflection,
  Scale,
  StructuredOutput,
} from "./api/types";

/** 1 つの問いのフォーム状態。使う項目は問いの種類で決まる。 */
export interface QuestionForm {
  // condition
  energy?: number;
  mood?: number;
  stress?: number;
  note: string;
  // concern / free
  text: string;
  intensity?: number;
  // reframe
  event: string;
  belief: string;
  emotion: string;
  emotion_intensity?: number;
  alternative: string;
}

export const SCALE_LABELS: Record<Scale, string> = { energy: "エネルギー", mood: "気分", stress: "ストレス" };

/** 保存済みの構造化出力を読む。無い・壊れている場合は null（従来の Markdown 表示にする）。 */
export function parseStructured(r: Pick<Reflection, "structured_json">): StructuredOutput | null {
  if (!r.structured_json) return null;
  try {
    const v = JSON.parse(r.structured_json) as StructuredOutput;
    return Array.isArray(v?.questions) && v.questions.length > 0 ? v : null;
  } catch {
    return null;
  }
}

/** 保存済みの回答の対応表。無ければ空。 */
export function parseAnswered(r: Pick<Reflection, "answers_json">): AnswersRecord {
  if (!r.answers_json) return { items: [] };
  try {
    const v = JSON.parse(r.answers_json) as AnswersRecord;
    return Array.isArray(v?.items) ? v : { items: [] };
  } catch {
    return { items: [] };
  }
}

/** 問いごとのフォームの初期状態。気になったことの強度は 3、捉え直しの A は AI の候補を入れておく（編集できる）。 */
export function initialForm(q: Question): QuestionForm {
  return {
    note: "",
    text: "",
    intensity: q.kind === "concern" ? 3 : undefined,
    event: q.kind === "reframe" ? (q.event ?? "") : "",
    belief: "",
    emotion: "",
    alternative: "",
  };
}

export function initialForms(out: StructuredOutput): QuestionForm[] {
  return out.questions.map(initialForm);
}

const blank = (s: string) => s.trim() === "";

/** 何も答えていないか（空のフォームは送らない）。捉え直しの A が候補のままなら、他に入力が無い限り空とみなす。 */
export function isBlank(q: Question, f: QuestionForm): boolean {
  switch (q.kind) {
    case "condition":
      return f.energy == null && f.mood == null && f.stress == null && blank(f.note);
    case "concern":
      return blank(f.text);
    case "free":
      return blank(f.text);
    case "reframe": {
      const aUntouched = blank(f.event) || f.event.trim() === (q.event ?? "").trim();
      return aUntouched && blank(f.belief) && blank(f.emotion) && f.emotion_intensity == null && blank(f.alternative);
    }
  }
}

const opt = (s: string) => (blank(s) ? undefined : s.trim());

/** フォーム 1 件を回答に変換する（尋ねていない尺度は送らない）。 */
export function toAnswerItem(index: number, q: Question, f: QuestionForm): AnswerItem {
  switch (q.kind) {
    case "condition": {
      const asked = new Set<Scale>(q.scales && q.scales.length > 0 ? q.scales : ["energy", "mood", "stress"]);
      return {
        index,
        energy: asked.has("energy") ? f.energy : undefined,
        mood: asked.has("mood") ? f.mood : undefined,
        stress: asked.has("stress") ? f.stress : undefined,
        note: opt(f.note),
      };
    }
    case "concern":
      return { index, text: opt(f.text), intensity: f.intensity };
    case "free":
      return { index, text: opt(f.text) };
    case "reframe":
      return {
        index,
        event: opt(f.event),
        belief: opt(f.belief),
        emotion: opt(f.emotion),
        emotion_intensity: f.emotion_intensity,
        alternative: opt(f.alternative),
      };
  }
}

/**
 * フォーム状態から回答 JSON を作る。回答済みの問い（`answered`）と空のフォームは含めない。
 * undefined の項目は JSON にした時点で落ちる。
 */
export function buildAnswers(
  questions: Question[],
  forms: QuestionForm[],
  answered: ReadonlySet<number> = new Set(),
): AnswersInput {
  const answers: AnswerItem[] = [];
  questions.forEach((q, i) => {
    const f = forms[i];
    if (!f || answered.has(i) || isBlank(q, f)) return;
    answers.push(toAnswerItem(i, q, f));
  });
  return { answers };
}

/** 送れる回答が 1 件でもあるか。 */
export function hasAnswers(questions: Question[], forms: QuestionForm[], answered: ReadonlySet<number>): boolean {
  return buildAnswers(questions, forms, answered).answers.length > 0;
}

/** 回答済みの問い 1 件の、保存した内容を「ラベル: 値」の行にする（記録が削除済みなら空）。 */
export function detailLines(d: AnswerDetail): string[] {
  const lines: string[] = [];
  const add = (label: string, v: string | number | null | undefined) => {
    if (v != null && String(v).trim() !== "") lines.push(`${label}: ${v}`);
  };
  if (d.condition) {
    const c = d.condition;
    add(SCALE_LABELS.energy, c.energy);
    add(SCALE_LABELS.mood, c.mood);
    add(SCALE_LABELS.stress, c.stress);
    add("ERG タグ", c.erg_tag ? ERG_NAMES[c.erg_tag] : null);
    add("メモ", c.note);
  }
  if (d.concern) {
    add("気になったこと", d.concern.text);
    add("強さ", d.concern.intensity);
  }
  if (d.reframe) {
    const a = d.reframe;
    add("A: 出来事", a.event);
    add("B: 受け止め方", a.belief);
    add("C: 感情", a.emotion);
    add("感情の強さ", a.emotion_intensity);
    add("別の捉え方", a.alternative);
  }
  return lines;
}

/** 尋ねる尺度（空なら 3 つとも）。 */
export function askedScales(q: Question): Scale[] {
  return q.scales && q.scales.length > 0 ? q.scales : ["energy", "mood", "stress"];
}

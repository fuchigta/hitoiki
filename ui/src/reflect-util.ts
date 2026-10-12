// 振り返りの構造化された問いに答えるフォームの、状態と回答 JSON の相互変換（純粋関数）。

import type {
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
  // checkin
  energy?: number;
  mood?: number;
  stress?: number;
  note: string;
  // hassle / free
  text: string;
  intensity?: number;
  // abc
  activating_event: string;
  belief: string;
  emotion: string;
  emotion_intensity?: number;
  dispute: string;
}

export const SCALE_LABELS: Record<Scale, string> = { energy: "元気", mood: "気分", stress: "ストレス" };

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

/** 問いごとのフォームの初期状態。ハッスルの強度は 3、ABC の A は AI の候補を入れておく（編集できる）。 */
export function initialForm(q: Question): QuestionForm {
  return {
    note: "",
    text: "",
    intensity: q.kind === "hassle" ? 3 : undefined,
    activating_event: q.kind === "abc" ? (q.activating_event ?? "") : "",
    belief: "",
    emotion: "",
    dispute: "",
  };
}

export function initialForms(out: StructuredOutput): QuestionForm[] {
  return out.questions.map(initialForm);
}

const blank = (s: string) => s.trim() === "";

/** 何も答えていないか（空のフォームは送らない）。ABC の A が候補のままなら、他に入力が無い限り空とみなす。 */
export function isBlank(q: Question, f: QuestionForm): boolean {
  switch (q.kind) {
    case "checkin":
      return f.energy == null && f.mood == null && f.stress == null && blank(f.note);
    case "hassle":
      return blank(f.text);
    case "free":
      return blank(f.text);
    case "abc": {
      const aUntouched = blank(f.activating_event) || f.activating_event.trim() === (q.activating_event ?? "").trim();
      return aUntouched && blank(f.belief) && blank(f.emotion) && f.emotion_intensity == null && blank(f.dispute);
    }
  }
}

const opt = (s: string) => (blank(s) ? undefined : s.trim());

/** フォーム 1 件を回答に変換する（尋ねていない尺度は送らない）。 */
export function toAnswerItem(index: number, q: Question, f: QuestionForm): AnswerItem {
  switch (q.kind) {
    case "checkin": {
      const asked = new Set<Scale>(q.scales && q.scales.length > 0 ? q.scales : ["energy", "mood", "stress"]);
      return {
        index,
        energy: asked.has("energy") ? f.energy : undefined,
        mood: asked.has("mood") ? f.mood : undefined,
        stress: asked.has("stress") ? f.stress : undefined,
        note: opt(f.note),
      };
    }
    case "hassle":
      return { index, text: opt(f.text), intensity: f.intensity };
    case "free":
      return { index, text: opt(f.text) };
    case "abc":
      return {
        index,
        activating_event: opt(f.activating_event),
        belief: opt(f.belief),
        emotion: opt(f.emotion),
        emotion_intensity: f.emotion_intensity,
        dispute: opt(f.dispute),
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

/** 尋ねる尺度（空なら 3 つとも）。 */
export function askedScales(q: Question): Scale[] {
  return q.scales && q.scales.length > 0 ? q.scales : ["energy", "mood", "stress"];
}

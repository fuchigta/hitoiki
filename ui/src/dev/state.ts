// モックのメモリ上の状態と、シナリオ別のフィクスチャ（日本語の現実的なデータ）。
// 開発専用。本番ビルドには含まれない。

import type * as T from "../api/types";
import { BASE_NOW, DAY, MIN, at, dayStart, weekdayMon0 } from "./time";

export type Scenario = "default" | "empty" | "busy";

export const SCENARIOS: Scenario[] = ["default", "empty", "busy"];

/** タスク + スキル名（core の task_skill に相当）。 */
export type TaskRec = T.Task & { skills: string[] };

export type SegRec = Omit<T.SegmentView, "start" | "end" | "duration_ms" | "task_title">;

export interface CorrectionRec {
  id: string;
  start_at: number;
  end_at: number;
  task_id: string | null;
  kind: T.SegmentKind | null;
  activity_label: string | null;
  total_ms: number;
  offline_ms: number;
  segment_count: number;
  created_at: number;
  /** 取り消し用: 変更前の区間の状態。 */
  before: SegRec[];
}

export interface State {
  scenario: Scenario;
  now: number;
  seq: number;
  tasks: TaskRec[];
  taskRules: T.TaskMatchRule[];
  doingSince: Record<string, number>;
  previousDoing: string | null;
  segments: SegRec[];
  blocks: T.TimeBlock[];
  templates: T.TemplateView[];
  appliedTemplates: Record<string, string[]>;
  checkins: T.Checkin[];
  hassles: T.Hassle[];
  abcs: T.AbcRecord[];
  objectives: T.Objective[];
  krs: T.KeyResult[];
  krSkills: Record<string, string[]>;
  krProgress: T.KrProgressEntry[];
  skills: T.Skill[];
  activityRules: T.ActivityRule[];
  activityLabels: string[];
  ergLabels: T.ErgLabel[];
  reflections: T.Reflection[];
  corrections: CorrectionRec[];
  calendarSources: T.CalendarSourceView[];
  calendarEvents: T.CalendarEvent[];
  ignoreExes: string[];
  ignoreCustomized: boolean;
  ruleSuggestions: T.RuleSuggestion[];
  ignoreSuggestions: T.IgnoreSuggestion[];
  pushes: T.PushView[];
  pendingPush: T.PushEvent | null;
  snoozeUntil: number;
  hints: Set<string>;
  onboarding: { completed: boolean; completed_at: number | null };
  config: Record<string, unknown>;
  quickShortcut: string;
}

export const DEFAULT_IGNORE_EXES = ["SearchHost.exe", "ShellExperienceHost.exe", "LockApp.exe"];

export function nid(s: State, prefix = "m"): string {
  s.seq += 1;
  return `${prefix}-${String(s.seq).padStart(4, "0")}`;
}

/** 決定的な疑似乱数（mulberry32）。 */
function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function baseConfig(): Record<string, unknown> {
  return {
    general: { timezone: "Asia/Tokyo", autostart: true, update_check: null, quick_entry_shortcut: "Ctrl+Alt+Space" },
    push: {
      enabled: true,
      idle_return_min: 15,
      urgent_task_interval_min: 120,
      important_stale_days: 3,
      kr_idle_weekday: 3,
    },
    review: { trust_doing: false, auto_break_locked_min: 10, min_sec: 60, snap_sec: 120 },
    collect: { ignore_exes: DEFAULT_IGNORE_EXES },
    calendar: { sources: [] as unknown[] },
    activity: { labels: [] as string[] },
  };
}

export function emptyState(scenario: Scenario): State {
  return {
    scenario,
    now: BASE_NOW,
    seq: 0,
    tasks: [],
    taskRules: [],
    doingSince: {},
    previousDoing: null,
    segments: [],
    blocks: [],
    templates: [],
    appliedTemplates: {},
    checkins: [],
    hassles: [],
    abcs: [],
    objectives: [],
    krs: [],
    krSkills: {},
    krProgress: [],
    skills: [],
    activityRules: [],
    activityLabels: [],
    ergLabels: [],
    reflections: [],
    corrections: [],
    calendarSources: [],
    calendarEvents: [],
    ignoreExes: [...DEFAULT_IGNORE_EXES],
    ignoreCustomized: false,
    ruleSuggestions: [],
    ignoreSuggestions: [],
    pushes: [],
    pendingPush: null,
    snoozeUntil: 0,
    hints: new Set(),
    onboarding: { completed: false, completed_at: null },
    config: baseConfig(),
    quickShortcut: "Ctrl+Alt+Space",
  };
}

// ===== フィクスチャ =====

const D0 = BASE_NOW;

function mkTask(s: State, o: Partial<TaskRec> & { title: string }): TaskRec {
  const t: TaskRec = {
    id: nid(s, "task"),
    title: o.title,
    body_md: o.body_md ?? "",
    status: o.status ?? "todo",
    kind: o.kind ?? "normal",
    urgent: o.urgent ?? null,
    important: o.important ?? null,
    estimate_min: o.estimate_min ?? null,
    planned_start_at: o.planned_start_at ?? null,
    planned_end_at: o.planned_end_at ?? null,
    key_result_id: o.key_result_id ?? null,
    external_url: o.external_url ?? null,
    created_at: o.created_at ?? D0 - 9 * DAY,
    updated_at: o.updated_at ?? D0 - DAY,
    done_at: o.done_at ?? null,
    skills: o.skills ?? [],
  };
  s.tasks.push(t);
  return t;
}

function mkSkill(s: State, name: string, parent?: string, description?: string): T.Skill {
  const p = parent ? s.skills.find((x) => x.name === parent) : undefined;
  const k: T.Skill = { id: nid(s, "skill"), name, parent_id: p?.id ?? null, description: description ?? null, preset: false };
  s.skills.push(k);
  return k;
}

function seedSkills(s: State) {
  mkSkill(s, "プログラミング");
  mkSkill(s, "Rust", "プログラミング", "所有権・ライフタイム・非同期");
  mkSkill(s, "TypeScript", "プログラミング");
  mkSkill(s, "設計", undefined, "アーキテクチャ・API・データモデル");
  mkSkill(s, "文章", undefined, "設計書・報告資料");
  mkSkill(s, "会議運営");
}

function seedOkr(s: State) {
  const o1: T.Objective = {
    id: nid(s, "obj"),
    title: "今期は設計力を伸ばす",
    description_md: "レビューで指摘される点を減らし、設計書を自分で仕上げられるようにする。",
    period_start: at(-30, "00:00"),
    period_end: at(60, "00:00"),
    status: "active",
  };
  const o2: T.Objective = {
    id: nid(s, "obj"),
    title: "無理なく働き続ける",
    description_md: null,
    period_start: null,
    period_end: null,
    status: "active",
  };
  s.objectives.push(o1, o2);
  const kr = (objective: T.Objective, title: string, extra: Partial<T.KeyResult>): T.KeyResult => {
    const k: T.KeyResult = {
      id: nid(s, "kr"),
      objective_id: objective.id,
      title,
      target: null,
      current: null,
      status: "active",
      measure: "time",
      day_min: 30,
      ...extra,
    };
    s.krs.push(k);
    return k;
  };
  const k1 = kr(o1, "設計書を 3 本仕上げる", { target: "3 本", current: "1 本", measure: "done" });
  const k2 = kr(o1, "Rust の学習に週 5 時間使う", { target: "5 時間/週", measure: "time" });
  const k3 = kr(o1, "設計レビューに週 2 回参加する", { target: "週 2 回", measure: "days", day_min: 20 });
  const k4 = kr(o2, "毎日 12 時台に休憩を取る", { target: "週 5 日", measure: "days", day_min: 15 });
  s.krSkills[k2.id] = ["Rust"];
  s.krSkills[k3.id] = ["設計"];
  s.krProgress.push(
    { id: nid(s, "prog"), kr_id: k1.id, value: 0, text: "0 本", note: "着手前", at: at(-28, "10:00") },
    { id: nid(s, "prog"), kr_id: k1.id, value: 1, text: "1 本", note: "API 設計書が完了", at: at(-9, "17:30") },
  );
  return { k1, k2, k3, k4 };
}

function seedTemplates(s: State, tasks: TaskRec[]) {
  const item = (id: string, start: string, end: string, label: string | null, focus: boolean, task?: TaskRec): T.TemplateItemView => {
    const [sh, sm] = start.split(":").map(Number);
    const [eh, em] = end.split(":").map(Number);
    return {
      id,
      start,
      end,
      start_min: sh * 60 + sm,
      end_min: eh * 60 + em,
      label,
      focus,
      task_id: task?.id ?? null,
      task_title: task?.title ?? null,
    };
  };
  s.templates.push(
    {
      id: nid(s, "tpl"),
      name: "開発の日",
      description: "午前に集中、午後は打ち合わせとレビュー",
      weekdays: [1, 2, 3, 4],
      created_at: D0 - 20 * DAY,
      updated_at: D0 - 20 * DAY,
      blocks: [
        item("ti1", "09:00", "09:15", "朝会", false),
        item("ti2", "09:15", "11:45", "集中: 設計", true, tasks[1]),
        item("ti3", "13:00", "15:00", "実装", true),
        item("ti4", "15:00", "16:00", "レビュー・打ち合わせ", false),
        item("ti5", "16:00", "17:30", "事務・メール", false),
      ],
    },
    {
      id: nid(s, "tpl"),
      name: "金曜の振り返りデー",
      description: null,
      weekdays: [5],
      created_at: D0 - 20 * DAY,
      updated_at: D0 - 20 * DAY,
      blocks: [
        item("tj1", "09:30", "11:00", "週報の作成", true),
        item("tj2", "13:00", "15:00", "学習: Rust", true, tasks[3]),
      ],
    },
  );
}

type Chunk = { min: number; kind: T.SegmentKind; exe?: string; title?: string; task?: "pick" | "none"; status?: T.SegmentStatus; agent?: boolean };

const EXE_TITLES: Record<string, string[]> = {
  "Code.exe": ["main.rs - hitoiki - Visual Studio Code", "App.tsx - hitoiki - Visual Studio Code", "design.md - docs - Visual Studio Code"],
  "chrome.exe": ["Rust の所有権 - The Rust Programming Language", "Pull request #214 · fuchigta/hitoiki", "社内 Wiki - 設計ガイドライン"],
  "WindowsTerminal.exe": ["pwsh - cargo test", "pwsh - git log"],
  "slack.exe": ["開発チーム | Slack"],
  "Teams.exe": ["週次定例 | Microsoft Teams"],
  "EXCEL.EXE": ["見積もり_v3.xlsx - Excel"],
};

/** 1 日分の区間を作る。 */
function genDay(s: State, dayOffset: number, r: () => number, opts: { reviewEvery: number; until?: number; picks: TaskRec[] }) {
  const base = dayStart(BASE_NOW) + dayOffset * DAY;
  const wd = weekdayMon0(base);
  if (wd >= 5 && r() < 0.8) return; // 週末はほぼ記録なし
  let cur = base + 9 * 60 * MIN + Math.floor(r() * 20) * MIN;
  const end = opts.until ?? base + 18 * 60 * MIN;
  const exes = Object.keys(EXE_TITLES);
  let n = 0;
  while (cur < end) {
    const lunch = cur >= base + 12 * 60 * MIN && cur < base + 13 * 60 * MIN;
    let chunk: Chunk;
    if (lunch) {
      chunk = { min: 55, kind: "break", task: "none" };
    } else {
      const p = r();
      if (p < 0.5) chunk = { min: 20 + Math.floor(r() * 50), kind: "work", exe: r() < 0.6 ? "Code.exe" : "chrome.exe", task: "pick" };
      else if (p < 0.65) chunk = { min: 15 + Math.floor(r() * 25), kind: "agent_dialog", exe: "WindowsTerminal.exe", title: "claude", task: "pick", agent: true };
      else if (p < 0.75) chunk = { min: 10 + Math.floor(r() * 15), kind: "agent_watch", exe: "WindowsTerminal.exe", title: "claude", task: "pick", agent: true };
      else if (p < 0.85) chunk = { min: 15 + Math.floor(r() * 30), kind: "meeting", exe: "Teams.exe", task: "none" };
      else if (p < 0.92) chunk = { min: 5 + Math.floor(r() * 10), kind: "idle", task: "none" };
      else chunk = { min: 8 + Math.floor(r() * 15), kind: "work", exe: exes[Math.floor(r() * exes.length)], task: "none", status: "needs_review" };
    }
    n += 1;
    if (opts.reviewEvery > 0 && n % opts.reviewEvery === 0 && !lunch) {
      chunk = { min: 6 + Math.floor(r() * 20), kind: "work", exe: exes[Math.floor(r() * exes.length)], task: "none", status: "needs_review" };
    }
    const segEnd = Math.min(cur + chunk.min * MIN, end);
    const exe = chunk.exe;
    const titles = exe ? EXE_TITLES[exe] ?? [exe] : [];
    const task = chunk.task === "pick" ? opts.picks[Math.floor(r() * opts.picks.length)] : null;
    const needs = chunk.status === "needs_review";
    s.segments.push({
      id: nid(s, "seg"),
      start_at: cur,
      end_at: segEnd,
      kind: chunk.kind,
      task_id: needs ? null : task?.id ?? null,
      activity_label: chunk.kind === "meeting" ? "会議" : chunk.kind === "break" ? "休憩" : null,
      confidence: needs ? 0.35 : task ? 0.7 + r() * 0.3 : null,
      decided_by: needs ? "heuristic" : task ? (r() < 0.5 ? "rule" : "doing") : "kind",
      status: needs ? "needs_review" : task && r() < 0.3 ? "confirmed" : "auto",
      agent_session_id: chunk.agent ? `sess-${dayOffset}-${n}` : null,
      exe_name: exe ?? null,
      title: chunk.title ?? (titles.length ? titles[Math.floor(r() * titles.length)] : null),
    });
    cur = segEnd;
  }
}

function seedCommon(s: State, busy: boolean) {
  seedSkills(s);
  const { k1, k2, k3, k4 } = seedOkr(s);
  s.activityLabels = ["設計", "実装", "レビュー", "会議", "調査", "学習", "考えごと", "離席", "休憩"];
  (s.config.activity as { labels: string[] }).labels = s.activityLabels;

  const t1 = mkTask(s, {
    title: "見積もりレビューの指摘対応",
    status: "doing",
    urgent: true,
    important: true,
    estimate_min: 90,
    body_md: "レビューで挙がった 5 件の指摘に対応する。\n\n- 工数の根拠を追記\n- リスクの欄を埋める",
    key_result_id: k3.id,
    skills: ["設計"],
    updated_at: at(0, "09:55"),
  });
  const t2 = mkTask(s, {
    title: "認証まわりの設計書ドラフト",
    important: true,
    estimate_min: 180,
    key_result_id: k1.id,
    skills: ["設計", "文章"],
    planned_start_at: at(0, "13:00"),
    planned_end_at: at(0, "16:00"),
    external_url: "https://example.com/wiki/auth-design",
  });
  const t3 = mkTask(s, { title: "週次報告の資料作成", urgent: true, estimate_min: 60, skills: ["文章"] });
  const t4 = mkTask(s, { title: "Rust 所有権の復習", important: true, estimate_min: 120, key_result_id: k2.id, skills: ["Rust"] });
  mkTask(s, { title: "障害対応: ログ調査", kind: "interrupt", urgent: true, status: "done", done_at: at(-1, "15:20"), updated_at: at(-1, "15:20") });
  mkTask(s, { title: "請求書の確認", status: "done", done_at: at(-2, "11:00"), updated_at: at(-2, "11:00") });
  mkTask(s, { title: "古い案件の棚卸し", status: "dropped", updated_at: at(-6, "10:00") });
  const t8 = mkTask(s, { title: "TypeScript の型パズルを読む", estimate_min: 45, skills: ["TypeScript"] });
  s.doingSince[t1.id] = at(0, "09:55");

  s.taskRules.push(
    { id: nid(s, "rule"), task_id: t1.id, field: "title", pattern: "*見積もり*", pattern_type: "glob", priority: 10, enabled: true },
    { id: nid(s, "rule"), task_id: t4.id, field: "url", pattern: "doc.rust-lang.org", pattern_type: "contains", priority: 5, enabled: true },
    { id: nid(s, "rule"), task_id: t2.id, field: "cwd", pattern: "^C:\\\\src\\\\auth-design", pattern_type: "regex", priority: 5, enabled: false },
  );
  s.activityRules.push(
    { id: nid(s, "arule"), field: "exe", pattern: "Teams.exe", pattern_type: "contains", activity_label: "会議", priority: 5, enabled: true },
    { id: nid(s, "arule"), field: "title", pattern: "*Pull request*", pattern_type: "glob", activity_label: "レビュー", priority: 3, enabled: true },
  );
  seedTemplates(s, [t1, t2, t3, t4]);

  // 区間: 過去 100 日 + 今日（10:30 まで）。今日は読みやすいように明示的に作る。
  const picks = [t1, t2, t3, t4, t8];
  const r = rng(20261013);
  for (let d = -100; d < 0; d++) genDay(s, d, r, { reviewEvery: busy && d >= -9 ? 3 : 0, picks });
  // 先週・今週に確認待ちを少し入れる（default でも「仕分け」に数件出る）。
  if (!busy) {
    const wk = s.segments.filter((x) => x.start_at >= at(-9, "00:00") && x.start_at < at(-1, "00:00") && x.status === "auto" && x.kind === "work");
    for (const x of wk.slice(0, 5)) {
      x.status = "needs_review";
      x.task_id = null;
      x.confidence = 0.4;
      x.decided_by = "heuristic";
    }
  }
  const seg = (start: string, end: string, o: Partial<SegRec>): SegRec => ({
    id: nid(s, "seg"),
    start_at: at(0, start),
    end_at: at(0, end),
    kind: "work",
    task_id: null,
    activity_label: null,
    confidence: 0.9,
    decided_by: "doing",
    status: "auto",
    agent_session_id: null,
    exe_name: null,
    title: null,
    ...o,
  });
  s.segments.push(
    seg("09:00", "09:40", { task_id: t1.id, activity_label: "設計", exe_name: "Code.exe", title: "estimate.md - hitoiki - Visual Studio Code", status: "confirmed", decided_by: "user" }),
    seg("09:40", "09:55", { kind: "meeting", activity_label: "会議", confidence: 1, decided_by: "rule", exe_name: "Teams.exe", title: "朝会 | Microsoft Teams" }),
    seg("09:55", "10:10", { task_id: t1.id, exe_name: "EXCEL.EXE", title: "見積もり_v3.xlsx - Excel", decided_by: "rule" }),
    seg("10:10", "10:18", { status: "needs_review", confidence: 0.35, decided_by: "heuristic", exe_name: "chrome.exe", title: "Rust の所有権 - The Rust Programming Language" }),
    seg("10:18", "10:30", { kind: "agent_dialog", task_id: t1.id, exe_name: "WindowsTerminal.exe", title: "claude", agent_session_id: "sess-today-1", decided_by: "doing" }),
  );
  s.blocks.push(
    { id: nid(s, "blk"), start_at: at(0, "09:00"), end_at: at(0, "09:15"), task_id: null, label: "朝会", focus: false },
    { id: nid(s, "blk"), start_at: at(0, "09:30"), end_at: at(0, "11:30"), task_id: t1.id, label: null, focus: true },
    { id: nid(s, "blk"), start_at: at(0, "13:00"), end_at: at(0, "16:00"), task_id: t2.id, label: "設計書を書く", focus: true },
    { id: nid(s, "blk"), start_at: at(0, "16:30"), end_at: at(0, "17:00"), task_id: t3.id, label: null, focus: false },
    { id: nid(s, "blk"), start_at: at(-1, "13:00"), end_at: at(-1, "15:00"), task_id: t2.id, label: null, focus: true },
  );
  s.calendarSources.push({ name: "仕事", type: "ics", host: "calendar.example.com" });
  (s.config.calendar as { sources: unknown[] }).sources = [{ name: "仕事", type: "ics" }];
  s.calendarEvents.push(
    { id: nid(s, "ev"), source: "仕事", external_id: "e1", start_at: at(0, "09:40"), end_at: at(0, "09:55"), all_day: false, title: "朝会", location: "Teams", is_busy: true, fetched_at: D0 - 30 * MIN },
    { id: nid(s, "ev"), source: "仕事", external_id: "e2", start_at: at(0, "14:00"), end_at: at(0, "15:00"), all_day: false, title: "設計レビュー（認証）", location: "会議室 3B", is_busy: true, fetched_at: D0 - 30 * MIN },
    { id: nid(s, "ev"), source: "仕事", external_id: "e3", start_at: at(0, "00:00"), end_at: at(1, "00:00"), all_day: true, title: "ノー残業デー", location: null, is_busy: false, fetched_at: D0 - 30 * MIN },
  );

  // 記録
  s.checkins.push(
    { id: nid(s, "chk"), at: at(0, "09:02"), energy: 4, mood: 4, stress: 2, erg_tag: null, note: "よく眠れた。", push_id: null },
    { id: nid(s, "chk"), at: at(-1, "17:40"), energy: 2, mood: 3, stress: 4, erg_tag: "relatedness", note: "レビューの返答待ちが多かった。", push_id: null },
    { id: nid(s, "chk"), at: at(-3, "09:10"), energy: 3, mood: 3, stress: 3, erg_tag: null, note: null, push_id: null },
  );
  const h1: T.Hassle = { id: nid(s, "has"), at: at(-1, "14:20"), text: "同じ質問を何度もチャットで聞かれて、作業が途切れた", intensity: 3, segment_id: null, push_id: null };
  s.hassles.push(h1, { id: nid(s, "has"), at: at(0, "10:12"), text: "見積もりの根拠資料がどこにあるか分からない", intensity: 2, segment_id: null, push_id: null });
  s.abcs.push({
    id: nid(s, "abc"),
    at: at(-1, "18:05"),
    activating_event: "レビューで設計の前提を指摘された",
    belief: "自分は設計が向いていないのかもしれない",
    consequence_emotion: "落ち込み",
    consequence_intensity: 3,
    dispute: null,
    ai_dialog_md: null,
    hassle_id: h1.id,
  });
  s.ergLabels.push(
    { record_kind: "checkin", record_id: s.checkins[1].id, tag: "relatedness", source: "user", confidence: null, reason: null, at: at(-1, "17:45") },
    { record_kind: "hassle", record_id: h1.id, tag: "relatedness", source: "suggested", confidence: 0.55, reason: "他の人とのやり取りに関する内容のため", at: at(-1, "14:25") },
    { record_kind: "abc", record_id: s.abcs[0].id, tag: "growth", source: "ai", confidence: 0.82, reason: "設計の力に関する悩みのため", at: at(-1, "18:10") },
  );
  s.reflections.push({
    id: nid(s, "ref"),
    at: at(-1, "17:55"),
    kind: "day_end",
    context_json: "{}",
    questions_md: "- 今日いちばん時間を使ったのは何でしたか。",
    answers_md: "見積もりレビューに時間を使った。思ったより資料集めに掛かった。",
    summary_md: "## 昨日のふりかえり\n\n設計とレビュー対応が中心の1日でした。",
    structured_json: null,
    answers_json: null,
  });

  // 通知
  const payload: T.PushPayload = {
    trigger: "checkin",
    title: "いまの調子はどうですか",
    body: "元気度をひとつ選んでください。答えなくても大丈夫です。",
    actions: [1, 2, 3, 4, 5].map((n) => ({ id: "energy", label: String(n), value: n })),
    input: { id: "hassle", placeholder: "困っていることがあれば一言" },
    open_screen: null,
  };
  s.pushes.push({ id: nid(s, "push"), at: at(0, "10:00"), at_local: "10:00", trigger: "checkin", trigger_label: "チェックイン", title: payload.title, body: payload.body, state: "unanswered", payload });
  s.pendingPush = { push_id: s.pushes[0].id, payload };

  s.ruleSuggestions.push({ task_id: t4.id, task_title: t4.title, field: "url", pattern: "doc.rust-lang.org", pattern_type: "contains", count: 3, target: "doc.rust-lang.org" });
  s.ignoreSuggestions.push({
    exe: "PowerToys.Run.exe",
    reasons: [{ code: "flicker", text: "短い区間が多い" }],
    span_count: 24,
    total_ms: 4 * MIN,
    short_count: 22,
    short_ratio: 0.92,
    review_count: 6,
    user_ignored_count: 0,
    user_no_task_count: 2,
    summary: "PowerToys.Run.exe は短い区間が 24 回あり、合計 4 分です。",
  });

  s.onboarding = { completed: true, completed_at: at(-30, "09:00") };
  s.hints.add("calendar");
  s.config.general = { ...(s.config.general as object), update_check: false };
  return { t1, t2, t3, t4, t8, k1, k2, k3, k4 };
}

function seedBusy(s: State, ctx: ReturnType<typeof seedCommon>) {
  const names = [
    "新入社員向けオンボーディング資料の改訂（セキュリティ・開発環境・申請手続き・社内ツールの使い方を含む全体の見直しと、旧資料との差分の洗い出し）",
    "クラウド費用の月次レビュー準備",
    "顧客 A 社向けの提案書の叩き台",
    "テスト自動化の方針整理",
    "CI が不安定な件の原因調査",
    "ドキュメントの誤字修正まとめ",
    "メンバーとの 1on1 の準備",
    "ライブラリの依存更新（minor のみ）",
    "障害振り返りの議事録作成",
    "ログ基盤の移行計画",
    "パフォーマンス計測のスクリプト作成",
    "採用面接の評価シート記入",
    "社内勉強会の発表準備",
    "コードレビュー（認証 API）",
    "コードレビュー（バッチ処理）",
    "請求データの突き合わせ",
    "監視アラートのしきい値見直し",
    "デザインシステムの命名ルール確認",
    "リリースノートの下書き",
    "経費精算",
    "SQL のスロークエリ調査",
    "KR の中間確認の資料づくり",
  ];
  names.forEach((title, i) => {
    mkTask(s, {
      title,
      urgent: i % 4 === 0 ? true : null,
      important: i % 3 === 0 ? true : null,
      estimate_min: i % 2 === 0 ? 30 + (i % 5) * 30 : null,
      kind: i === 9 ? "interrupt" : "normal",
      status: i === 5 ? "done" : i === 11 ? "dropped" : "todo",
      done_at: i === 5 ? at(-2, "16:00") : null,
      skills: i % 3 === 0 ? ["設計"] : i % 4 === 1 ? ["TypeScript", "Rust"] : [],
      key_result_id: i % 5 === 0 ? ctx.k1.id : null,
    });
  });
  for (const label of ["レビュー待ちの時間の使い方", "資料づくり（長期）"]) mkSkill(s, label);
  for (let i = 0; i < 6; i++) {
    const p = {
      trigger: "idle_return",
      title: "さっきの時間は何をしていましたか",
      body: `${10 + i}:00 から 20 分間、操作がありませんでした。`,
      actions: [
        { id: "break", label: "休憩", value: null },
        { id: "meeting", label: "会議", value: null },
        { id: "thinking", label: "考えごと", value: null },
      ],
      input: null,
      open_screen: "review",
    } satisfies T.PushPayload;
    s.pushes.push({ id: nid(s, "push"), at: at(0, `${String(8 + (i % 3)).padStart(2, "0")}:${String(10 * i).padStart(2, "0")}`), at_local: "", trigger: "idle_return", trigger_label: "席に戻ったとき", title: p.title, body: p.body, state: "unanswered", payload: p });
  }
  for (let i = 0; i < 8; i++) {
    const c: T.Checkin = { push_id: null, id: nid(s, "chk"), at: at(-i, "17:30"), energy: 1 + (i % 5), mood: 1 + ((i + 2) % 5), stress: 1 + ((i + 3) % 5), erg_tag: null, note: i % 2 ? "長い一日だった。" : null };
    s.checkins.push(c);
    s.ergLabels.push({ record_kind: "checkin", record_id: c.id, tag: "existence", source: "suggested", confidence: 0.5, reason: "体調に関する記録のため", at: c.at });
  }
  s.ruleSuggestions.push(
    { task_id: ctx.t2.id, task_title: ctx.t2.title, field: "exe", pattern: "EXCEL.EXE", pattern_type: "contains", count: 5, target: "EXCEL.EXE" },
    { task_id: ctx.t3.id, task_title: ctx.t3.title, field: "cwd", pattern: "C:\\work\\reports", pattern_type: "contains", count: 4, target: "C:\\work\\reports" },
  );
}

export function buildState(scenario: Scenario): State {
  const s = emptyState(scenario);
  if (scenario === "empty") return s;
  const ctx = seedCommon(s, scenario === "busy");
  if (scenario === "busy") seedBusy(s, ctx);
  return s;
}

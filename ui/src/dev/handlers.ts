// モックのコマンドハンドラ。`api/index.ts` の全コマンドに応答する（完全再現ではなく、画面で操作が反映される程度）。
// 新しい Tauri コマンドを `api/index.ts` に足したら、ここにも足すこと（`mock.test.ts` が網羅を検査する）。
// 開発専用。本番ビルドには含まれない。

import type * as T from "../api/types";
import { buildState, DEFAULT_IGNORE_EXES, nid, type CorrectionRec, type Scenario, type SegRec, type State, type TaskRec } from "./state";
import { BASE_NOW, DAY, HOUR, MIN, at, dayStart, isoJst, parseYmd, rangeView, resolveRange, weekStart, weekdayMon0, ymd } from "./time";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type A = Record<string, any>;
type Handler = (s: State, a: A, ctx: Ctx) => unknown;

interface Ctx {
  emit: (name: string, payload?: unknown) => void;
}

/** core の AppError 相当。 */
function fail(code: string, message: string): never {
  throw { code, message };
}

const COUNTED: T.SegmentKind[] = ["work", "agent_dialog", "agent_watch", "meeting", "interrupt"];

// ===== 変換 =====

function taskView(s: State, t: TaskRec): T.TaskView {
  const k = t.key_result_id ? s.krs.find((x) => x.id === t.key_result_id) : undefined;
  const o = k ? s.objectives.find((x) => x.id === k.objective_id) : undefined;
  return { ...t, skills: [...t.skills], key_result: k && o ? { id: k.id, title: k.title, objective_id: o.id, objective_title: o.title } : null };
}

function segView(s: State, r: SegRec): T.SegmentView {
  const t = r.task_id ? s.tasks.find((x) => x.id === r.task_id) : undefined;
  return { ...r, start: isoJst(r.start_at), end: isoJst(r.end_at), duration_ms: r.end_at - r.start_at, task_title: t?.title ?? null };
}

function findTask(s: State, q: string | undefined | null): TaskRec | undefined {
  if (!q) return undefined;
  return s.tasks.find((t) => t.id === q) ?? s.tasks.find((t) => t.title === q) ?? s.tasks.find((t) => t.title.includes(q));
}

function needTask(s: State, q: string | undefined | null): TaskRec {
  const t = findTask(s, q);
  if (!t) fail("not_found", `タスクが見つかりません: ${q ?? ""}`);
  return t;
}

function findKr(s: State, q: string | undefined | null): T.KeyResult | undefined {
  if (!q) return undefined;
  return s.krs.find((k) => k.id === q) ?? s.krs.find((k) => k.title === q);
}

function needKr(s: State, q: string): T.KeyResult {
  const k = findKr(s, q);
  if (!k) fail("not_found", `KR が見つかりません: ${q}`);
  return k;
}

function range(s: State, a?: T.RangeArgs): [number, number] {
  return resolveRange(a, s.now);
}

function overlap(r: { start_at: number; end_at: number }, from: number, to: number): number {
  return Math.max(0, Math.min(r.end_at, to) - Math.max(r.start_at, from));
}

function inRange(s: State, from: number, to: number): SegRec[] {
  return s.segments.filter((x) => x.end_at > from && x.start_at < to).sort((a, b) => a.start_at - b.start_at);
}

function fmtMin(ms: number): string {
  const m = Math.round(ms / MIN);
  return m < 60 ? `${m}分` : m % 60 === 0 ? `${m / 60}時間` : `${Math.floor(m / 60)}時間${m % 60}分`;
}

function rowsFrom(map: Map<string, { key: string | null; label: string; ms: number; n: number; breakdown?: Record<string, number> }>, total: number): T.ReportRow[] {
  return [...map.values()]
    .sort((a, b) => b.ms - a.ms)
    .map((v) => ({ key: v.key, label: v.label, duration_ms: v.ms, ratio: total > 0 ? v.ms / total : 0, segments: v.n, breakdown: v.breakdown }));
}

function group(segs: SegRec[], from: number, to: number, keyOf: (x: SegRec) => [string | null, string][]) {
  const map = new Map<string, { key: string | null; label: string; ms: number; n: number; breakdown: Record<string, number> }>();
  for (const x of segs) {
    const ms = overlap(x, from, to);
    for (const [key, label] of keyOf(x)) {
      const id = key ?? "-";
      const e = map.get(id) ?? { key, label, ms: 0, n: 0, breakdown: {} };
      e.ms += ms;
      e.n += 1;
      e.breakdown[x.kind] = (e.breakdown[x.kind] ?? 0) + ms;
      map.set(id, e);
    }
  }
  return map;
}

const KIND_LABELS: Record<T.SegmentKind, string> = {
  work: "作業",
  agent_dialog: "AI との対話",
  agent_watch: "AI の作業待ち",
  meeting: "会議",
  idle: "無操作",
  break: "休憩",
  interrupt: "割り込み",
  ignored: "無視",
};

function quadrant(t: TaskRec | undefined): [string | null, string] {
  if (!t) return [null, "（タスクなし）"];
  const u = t.urgent === true;
  const i = t.important === true;
  if (u && i) return ["ui", "緊急かつ重要"];
  if (i) return ["i", "重要（緊急ではない）"];
  if (u) return ["u", "緊急（重要ではない）"];
  return ["n", "どちらでもない"];
}

function skillClosure(s: State, names: string[]): Set<string> {
  const out = new Set<string>(names);
  let grew = true;
  while (grew) {
    grew = false;
    for (const sk of s.skills) {
      const parent = sk.parent_id ? s.skills.find((x) => x.id === sk.parent_id) : undefined;
      if (parent && out.has(parent.name) && !out.has(sk.name)) {
        out.add(sk.name);
        grew = true;
      }
    }
  }
  return out;
}

// ===== レポート =====

function buildReport(s: State, by: T.ReportBy, a: T.RangeArgs | undefined, includeIdle: boolean): T.Report {
  const [from, to] = range(s, a);
  const all = inRange(s, from, to);
  const counted = (k: T.SegmentKind) => COUNTED.includes(k) || (includeIdle && (k === "idle" || k === "break"));
  const segs = all.filter((x) => counted(x.kind));
  const total = segs.reduce((n, x) => n + overlap(x, from, to), 0);
  const task = (x: SegRec) => s.tasks.find((t) => t.id === x.task_id);
  let map;
  let multi = false;
  switch (by) {
    case "task":
      map = group(segs, from, to, (x) => [x.task_id ? [x.task_id, task(x)?.title ?? "（削除済み）"] : [null, "（タスクなし）"]]);
      break;
    case "activity":
      map = group(segs, from, to, (x) => [x.activity_label ? [x.activity_label, x.activity_label] : [null, "（活動なし）"]]);
      break;
    case "quadrant":
      map = group(segs, from, to, (x) => [quadrant(task(x))]);
      break;
    case "okr":
      map = group(segs, from, to, (x) => {
        const k = s.krs.find((kr) => kr.id === task(x)?.key_result_id);
        return [k ? [k.id, k.title] : [null, "（KR なし）"]];
      });
      break;
    case "skill":
      multi = true;
      map = group(segs, from, to, (x) => {
        const sk = task(x)?.skills ?? [];
        return sk.length ? sk.map((n): [string, string] => [n, n]) : [[null, "（スキルなし）"]];
      });
      break;
    case "agent":
      map = group(
        segs.filter((x) => x.agent_session_id),
        from,
        to,
        (x) => [[x.agent_session_id, `セッション ${x.agent_session_id}`]],
      );
      break;
    default:
      map = group(segs, from, to, (x) => [[x.kind, KIND_LABELS[x.kind]]]);
  }
  const sum = (kinds: T.SegmentKind[]) => all.filter((x) => kinds.includes(x.kind)).reduce((n, x) => n + overlap(x, from, to), 0);
  const rowsTotal = by === "skill" ? [...map.values()].reduce((n, v) => n + v.ms, 0) : total;
  return {
    range: rangeView(from, to),
    by,
    total_ms: total,
    rows: rowsFrom(map, rowsTotal),
    excluded: { idle_ms: includeIdle ? 0 : sum(["idle"]), break_ms: includeIdle ? 0 : sum(["break"]), ignored_ms: sum(["ignored"]) },
    include_idle: includeIdle,
    multi_attribution: multi,
    note: by === "agent" && map.size === 0 ? "この期間に AI エージェントの区間はありません。" : null,
  };
}

function todaySummary(s: State): T.TodaySummary {
  const [from, to] = range(s, { date: "today" });
  const r = buildReport(s, "task", { date: "today" }, false);
  const kinds = buildReport(s, "kind", { date: "today" }, false);
  const sum = (k: T.SegmentKind) => inRange(s, from, to).filter((x) => x.kind === k).reduce((n, x) => n + overlap(x, from, to), 0);
  return {
    range: rangeView(from, to),
    total_ms: r.total_ms,
    idle_ms: sum("idle"),
    break_ms: sum("break"),
    ignored_ms: sum("ignored"),
    by_kind: kinds.rows,
    top_tasks: r.rows.slice(0, 5),
  };
}

// ===== ヒートマップ =====

function buildHeatmap(s: State, by: string | undefined, fromS?: string, toS?: string): T.Heatmap {
  const today = dayStart(s.now);
  const to = toS ? parseYmd(toS) : today;
  const from = fromS ? parseYmd(fromS) : weekStart(to) - 52 * 7 * DAY;
  const [kind, arg] = (by ?? "time").split(":");
  const values = new Map<string, number>();
  const isCount = kind === "records";
  const add = (ms: number, v: number) => {
    const d = ymd(ms);
    values.set(d, (values.get(d) ?? 0) + v);
  };
  let label = "活動時間";
  if (isCount) {
    label = "記録の件数";
    const want = (k: string) => !arg || arg === k;
    if (want("checkin")) s.checkins.forEach((x) => add(x.at, 1));
    if (want("hassle")) s.hassles.forEach((x) => add(x.at, 1));
    if (want("abc")) s.abcs.forEach((x) => add(x.at, 1));
    if (want("reflection")) s.reflections.forEach((x) => add(x.at, 1));
  } else {
    const skillNames = kind === "skill" ? skillClosure(s, [arg]) : null;
    if (kind === "kind") label = `${KIND_LABELS[arg as T.SegmentKind] ?? arg}の時間`;
    if (kind === "task") label = `${s.tasks.find((t) => t.id === arg)?.title ?? "タスク"}の時間`;
    if (kind === "skill") label = `${arg} の時間`;
    if (kind === "kr") label = `${s.krs.find((k) => k.id === arg)?.title ?? "KR"}の時間`;
    for (const x of s.segments) {
      const t = s.tasks.find((y) => y.id === x.task_id);
      let ok = COUNTED.includes(x.kind);
      if (kind === "kind") ok = x.kind === arg;
      if (kind === "task") ok = ok && x.task_id === arg;
      if (kind === "skill") ok = ok && !!t?.skills.some((n) => skillNames!.has(n));
      if (kind === "kr") ok = ok && t?.key_result_id === arg;
      if (ok) add(x.start_at, x.end_at - x.start_at);
    }
  }
  const days: T.HeatmapDay[] = [];
  for (let d = from; d <= to; d += DAY) days.push({ date: ymd(d), value: values.get(ymd(d)) ?? 0, level: 0 });
  const nz = days.map((d) => d.value).filter((v) => v > 0).sort((a, b) => a - b);
  const q = (p: number) => nz[Math.min(nz.length - 1, Math.floor(nz.length * p))] ?? 0;
  const thresholds = [q(0.25), q(0.5), q(0.75)];
  for (const d of days) d.level = d.value <= 0 ? 0 : ((1 + thresholds.filter((t) => d.value > t).length) as 1 | 2 | 3 | 4);
  return {
    from: ymd(from),
    to: ymd(to),
    from_weekday: weekdayMon0(from),
    week_start: "monday",
    by: by ?? "time",
    label,
    unit: isCount ? "count" : "ms",
    days,
    total: nz.reduce((n, v) => n + v, 0),
    active_days: nz.length,
    max: nz.length ? nz[nz.length - 1] : 0,
    thresholds,
  };
}

// ===== OKR =====

function krRange(s: State, k: T.KeyResult, given?: T.RangeArgs): { from: number; to: number; source: T.KrSummary["range_source"] } {
  if (given && (given.date || given.from || given.to)) {
    const [from, to] = range(s, given);
    return { from, to, source: "given" };
  }
  const o = s.objectives.find((x) => x.id === k.objective_id);
  if (o?.period_start != null && o.period_end != null) return { from: o.period_start, to: Math.min(o.period_end, dayStart(s.now) + DAY), source: "objective" };
  const w = weekStart(s.now);
  return { from: w - 3 * 7 * DAY, to: w + 7 * DAY, source: "recent_weeks" };
}

function krSummary(s: State, k: T.KeyResult): T.KrSummary {
  const o = s.objectives.find((x) => x.id === k.objective_id)!;
  const { from, to, source } = krRange(s, k);
  const names = skillClosure(s, s.krSkills[k.id] ?? []);
  const linked = s.tasks.filter((t) => t.key_result_id === k.id);
  const hasSkill = (t?: TaskRec) => !!t?.skills.some((n) => names.has(n)) && (s.krSkills[k.id] ?? []).length > 0;
  let fromSkill = 0;
  let fromTask = 0;
  const perDay = new Map<string, number>();
  const perWeek = new Map<number, number>();
  const perSkill = new Map<string, number>();
  for (const x of inRange(s, from, to)) {
    if (!COUNTED.includes(x.kind)) continue;
    const t = s.tasks.find((y) => y.id === x.task_id);
    const bySkill = hasSkill(t);
    const byTask = t?.key_result_id === k.id;
    if (!bySkill && !byTask) continue;
    const ms = overlap(x, from, to);
    if (bySkill) fromSkill += ms;
    else fromTask += ms;
    perDay.set(ymd(x.start_at), (perDay.get(ymd(x.start_at)) ?? 0) + ms);
    perWeek.set(weekStart(x.start_at), (perWeek.get(weekStart(x.start_at)) ?? 0) + ms);
    for (const n of t?.skills ?? []) perSkill.set(n, (perSkill.get(n) ?? 0) + ms);
  }
  const weekly: T.KrWeekBar[] = [];
  for (let w = weekStart(from); w < to; w += 7 * DAY) weekly.push({ week_start: ymd(w), duration_ms: perWeek.get(w) ?? 0 });
  const tracked = (t: TaskRec) => s.segments.filter((x) => x.task_id === t.id).reduce((n, x) => n + (x.end_at - x.start_at), 0);
  const est = linked.filter((t) => t.estimate_min);
  return {
    key_result: k,
    objective: { id: o.id, title: o.title },
    range: rangeView(from, to),
    range_source: source,
    measure: k.measure,
    time_ms: fromSkill + fromTask,
    active_days: [...perDay.values()].filter((v) => v >= k.day_min * MIN).length,
    day_min: k.day_min,
    kr_skills: s.krSkills[k.id] ?? [],
    from_skill_ms: fromSkill,
    from_task_ms: fromTask,
    done_count: linked.filter((t) => t.status === "done").length,
    task_count: linked.length,
    weekly,
    skills: [...perSkill.entries()].map(([name, ms]) => ({ skill_id: s.skills.find((x) => x.name === name)?.id ?? null, name, duration_ms: ms })),
    estimate: {
      estimate_min_total: est.reduce((n, t) => n + (t.estimate_min ?? 0), 0),
      estimated_tasks: est.length,
      actual_ms_of_estimated: est.reduce((n, t) => n + tracked(t), 0),
    },
    tasks: linked.map((t) => ({ id: t.id, title: t.title, status: t.status, estimate_min: t.estimate_min, tracked_ms: tracked(t), done_at: t.done_at })),
  };
}

function numberOf(text: string): number | null {
  const m = /-?\d+(\.\d+)?/.exec(text);
  return m ? Number(m[0]) : null;
}

function krHistory(s: State, k: T.KeyResult): T.KrHistory {
  const { from, to, source } = krRange(s, k);
  const entries = s.krProgress.filter((x) => x.kr_id === k.id).sort((a, b) => b.at - a.at);
  const sum = krSummary(s, k);
  const weeks: T.KrHistoryWeek[] = sum.weekly.map((w) => {
    const ws = parseYmd(w.week_start);
    const upto = entries.find((e) => e.at < ws + 7 * DAY);
    return {
      week_start: w.week_start,
      duration_ms: w.duration_ms,
      value: upto?.value ?? null,
      text: upto?.text ?? null,
      changes: entries.filter((e) => e.at >= ws && e.at < ws + 7 * DAY).length,
    };
  });
  return { key_result: k, range: rangeView(from, to), range_source: source, weeks, entries };
}

// ===== 削除 =====

function deleteOutcome(entity: string, id: string, name: string, lines: T.ImpactLine[], mode: T.DeleteMode | undefined, doDelete: () => void, note: string | null = null): T.DeleteOutcome {
  const impact: T.Impact = { entity, id, name, lines: lines.filter((l) => l.count > 0), note };
  if (mode?.dry_run) return { dry_run: true, deleted: false, impact };
  doDelete();
  return { dry_run: false, deleted: true, impact };
}

// ===== 区間の操作 =====

function createdRule(s: State, seg: SegRec, taskId: string | null, activity: string | null): T.CreatedRule | null {
  if (!seg.exe_name) return null;
  if (taskId) {
    const dup = s.taskRules.find((r) => r.task_id === taskId && r.field === "exe" && r.pattern === seg.exe_name);
    const r: T.TaskMatchRule = dup ?? { id: nid(s, "rule"), task_id: taskId, field: "exe", pattern: seg.exe_name, pattern_type: "contains", priority: 0, enabled: true };
    if (!dup) s.taskRules.push(r);
    return { rule_type: "task_match_rule", id: r.id, field: r.field, pattern: r.pattern, pattern_type: r.pattern_type, task_id: taskId, activity_label: null, reused: !!dup };
  }
  if (activity) {
    const r: T.ActivityRule = { id: nid(s, "arule"), field: "exe", pattern: seg.exe_name, pattern_type: "contains", activity_label: activity, priority: 0, enabled: true };
    s.activityRules.push(r);
    return { rule_type: "activity_rule", id: r.id, field: r.field, pattern: r.pattern, pattern_type: r.pattern_type, task_id: null, activity_label: activity, reused: false };
  }
  return null;
}

function setSegment(s: State, id: string, input: { task?: string; kind?: T.SegmentKind; activity?: string; rule?: boolean }) {
  const seg = s.segments.find((x) => x.id === id);
  if (!seg) fail("not_found", `区間が見つかりません: ${id}`);
  const task = input.task ? needTask(s, input.task) : undefined;
  if (task) seg.task_id = task.id;
  if (input.kind) seg.kind = input.kind;
  if (input.activity) seg.activity_label = input.activity;
  seg.status = "confirmed";
  seg.decided_by = "user";
  seg.confidence = 1;
  const rule = input.rule ? createdRule(s, seg, task?.id ?? null, input.activity ?? null) : null;
  return { seg, rule, overrideId: nid(s, "ovr") };
}

function applyIdle(seg: SegRec, preset: T.IdlePreset) {
  switch (preset) {
    case "break":
      seg.kind = "break";
      break;
    case "meeting":
      seg.kind = "meeting";
      seg.activity_label = "会議";
      break;
    case "thinking":
      seg.kind = "work";
      seg.activity_label = "考えごと";
      break;
    case "away":
      seg.kind = "break";
      seg.activity_label = "離席";
      break;
  }
  seg.status = "confirmed";
  seg.decided_by = "user";
}

function startTask(s: State, t: TaskRec): T.TaskChange {
  const demoted: T.TaskView[] = [];
  for (const o of s.tasks) {
    if (o.id !== t.id && o.status === "doing") {
      o.status = "todo";
      delete s.doingSince[o.id];
      s.previousDoing = o.id;
      demoted.push(taskView(s, o));
    }
  }
  t.status = "doing";
  t.updated_at = s.now;
  s.doingSince[t.id] = s.now;
  return { task: taskView(s, t), demoted };
}

function applyTaskInput(s: State, t: TaskRec, i: T.TaskInput) {
  if (i.title !== undefined) t.title = i.title;
  if (i.body !== undefined) t.body_md = i.body;
  if (i.urgent !== undefined) t.urgent = i.urgent;
  if (i.important !== undefined) t.important = i.important;
  if (i.estimate_min !== undefined) t.estimate_min = i.estimate_min;
  if (i.key_result) t.key_result_id = needKr(s, i.key_result).id;
  if (i.clear_key_result) t.key_result_id = null;
  if (i.skills) t.skills = [...i.skills];
  if (i.clear_skills) t.skills = [];
  if (i.external_url !== undefined) t.external_url = i.external_url || null;
  if (i.planned_start_at !== undefined) t.planned_start_at = i.planned_start_at;
  if (i.planned_end_at !== undefined) t.planned_end_at = i.planned_end_at;
  if (i.interrupt !== undefined) t.kind = i.interrupt ? "interrupt" : "normal";
  t.updated_at = s.now;
}

function trackedMs(s: State, taskId: string): number {
  return s.segments.filter((x) => x.task_id === taskId).reduce((n, x) => n + (x.end_at - x.start_at), 0);
}

function deletePreview(s: State, t: TaskRec): T.TaskDeletePreview {
  const segs = s.segments.filter((x) => x.task_id === t.id);
  const rules = s.taskRules.filter((r) => r.task_id === t.id).length;
  const blocks = s.blocks.filter((b) => b.task_id === t.id).length;
  const isDoing = t.status === "doing";
  const tracked = segs.reduce((n, x) => n + (x.end_at - x.start_at), 0);
  return {
    task_id: t.id,
    title: t.title,
    status: t.status,
    segments: segs.length,
    tracked_ms: tracked,
    overrides: segs.filter((x) => x.decided_by === "user").length,
    corrections: s.corrections.filter((c) => c.task_id === t.id).length,
    rules,
    skills: t.skills.length,
    blocks,
    template_items: s.templates.reduce((n, tp) => n + tp.blocks.filter((b) => b.task_id === t.id).length, 0),
    doing_logs: s.doingSince[t.id] ? 1 : 0,
    is_doing: isDoing,
    needs_force: segs.length > 0 || isDoing,
    summary: `「${t.title}」を消すと、区間 ${segs.length} 件（${fmtMin(tracked)}）の割り当てが外れ、ルール ${rules} 件・計画ブロック ${blocks} 件が消えます。`,
  };
}

// ===== 実績の補正 =====

function candidatesFor(s: State, from: number, to: number): T.CorrectionCandidate[] {
  const segs = inRange(s, from, to);
  const out: T.CorrectionCandidate[] = [];
  const mk = (o: Partial<T.CorrectionCandidate> & { id: string; start_at: number; end_at: number; full: number; category: T.CorrectionCategory }): T.CorrectionCandidate => ({
    gap: false,
    start: isoJst(o.start_at),
    end: isoJst(o.end_at),
    duration_ms: o.end_at - o.start_at,
    full_duration_ms: o.full,
    kind: null,
    status: null,
    task_id: null,
    task_title: null,
    activity_label: null,
    exe_name: null,
    title: null,
    offline: false,
    becomes_offline: o.category === "idle" || o.category === "gap",
    checked: o.category === "work" || o.category === "agent_dialog",
    ...o,
  });
  let cursor = from;
  for (const x of segs) {
    const st = Math.max(x.start_at, from);
    const en = Math.min(x.end_at, to);
    if (st > cursor + MIN) out.push(mk({ id: `gap:${cursor}`, gap: true, start_at: cursor, end_at: st, full: st - cursor, category: "gap" }));
    const category: T.CorrectionCategory =
      x.status === "confirmed" ? "confirmed" : x.kind === "meeting" ? "meeting" : x.kind === "agent_dialog" || x.kind === "agent_watch" ? "agent_dialog" : x.kind === "idle" || x.kind === "break" || x.kind === "ignored" ? "idle" : "work";
    out.push(
      mk({
        id: x.id,
        start_at: st,
        end_at: en,
        full: x.end_at - x.start_at,
        category,
        kind: x.kind,
        status: x.status,
        task_id: x.task_id,
        task_title: s.tasks.find((t) => t.id === x.task_id)?.title ?? null,
        activity_label: x.activity_label,
        exe_name: x.exe_name,
        title: x.title,
        offline: !!x.offline,
      }),
    );
    cursor = Math.max(cursor, en);
  }
  if (to > cursor + MIN) out.push(mk({ id: `gap:${cursor}`, gap: true, start_at: cursor, end_at: to, full: to - cursor, category: "gap" }));
  return out;
}

function correctionTarget(s: State, i: T.CorrectionInput): string {
  if (i.rest) return "休憩";
  if (i.no_task) return "タスクなし";
  const t = i.task ? findTask(s, i.task) : undefined;
  return t ? `「${t.title}」` : "（未指定）";
}

function correctionPreview(s: State, i: T.CorrectionInput): T.CorrectionPreview {
  const cands = candidatesFor(s, i.start_at, i.end_at);
  const sel = i.selected ? new Set(i.selected) : new Set(cands.filter((c) => c.checked).map((c) => c.id));
  const chosen = cands.filter((c) => sel.has(c.id));
  const total = chosen.reduce((n, c) => n + c.duration_ms, 0);
  const off = chosen.filter((c) => c.becomes_offline && !i.rest).reduce((n, c) => n + c.duration_ms, 0);
  const target = correctionTarget(s, i);
  return {
    range: rangeView(i.start_at, i.end_at),
    target,
    selected_count: chosen.length,
    total_ms: total,
    offline_ms: off,
    excluded_count: cands.length - chosen.length,
    summary: `${target} に ${fmtMin(total)}を割り当て${off > 0 ? `（うち PC 外 ${fmtMin(off)}）` : ""}`,
    candidates: cands.map((c) => ({ ...c, checked: sel.has(c.id) })),
  };
}

function splitSegment(s: State, seg: SegRec, from: number, to: number): SegRec {
  const a = Math.max(from, seg.start_at);
  const b = Math.min(to, seg.end_at);
  if (a > seg.start_at) {
    s.segments.push({ ...seg, id: nid(s, "seg"), end_at: a });
    seg.start_at = a;
  }
  if (b < seg.end_at) {
    s.segments.push({ ...seg, id: nid(s, "seg"), start_at: b });
    seg.end_at = b;
  }
  return seg;
}

function correctionView(s: State, c: CorrectionRec): T.CorrectionView {
  return {
    id: c.id,
    start_at: c.start_at,
    end_at: c.end_at,
    start: isoJst(c.start_at),
    end: isoJst(c.end_at),
    task_id: c.task_id,
    task_title: s.tasks.find((t) => t.id === c.task_id)?.title ?? null,
    kind: c.kind,
    activity_label: c.activity_label,
    total_ms: c.total_ms,
    offline_ms: c.offline_ms,
    segment_count: c.segment_count,
    created_at: c.created_at,
    created: isoJst(c.created_at),
  };
}

// ===== 計画 =====

function blockView(s: State, b: T.TimeBlock): T.BlockView {
  return { ...b, task_title: s.tasks.find((t) => t.id === b.task_id)?.title ?? null };
}

function blockFromInput(s: State, i: T.BlockInput, id: string): T.TimeBlock {
  return { id, start_at: i.start, end_at: i.end, task_id: i.task ? needTask(s, i.task).id : null, label: i.label || null, focus: !!i.focus };
}

function hhmm(min: number): string {
  return `${String(Math.floor(min / 60)).padStart(2, "0")}:${String(min % 60).padStart(2, "0")}`;
}

function parseHm(v: string): number {
  const [h, m] = v.split(":").map(Number);
  return h * 60 + m;
}

function templateFromInput(s: State, i: T.TemplateInput, id: string, created_at: number): T.TemplateView {
  return {
    id,
    name: i.name,
    description: i.description ?? null,
    weekdays: [...i.weekdays],
    created_at,
    updated_at: s.now,
    blocks: i.items.map((it, n) => {
      const task = it.task ? findTask(s, it.task) : undefined;
      return {
        id: nid(s, `ti${n}`),
        start: it.start,
        end: it.end,
        start_min: parseHm(it.start),
        end_min: parseHm(it.end),
        label: it.label ?? null,
        focus: !!it.focus,
        task_id: task?.id ?? null,
        task_title: task?.title ?? null,
      };
    }),
  };
}

function needTemplate(s: State, q: string): T.TemplateView {
  const t = s.templates.find((x) => x.id === q) ?? s.templates.find((x) => x.name === q);
  if (!t) fail("not_found", `テンプレートが見つかりません: ${q}`);
  return t;
}

function dateOf(s: State, d?: string): number {
  if (!d || d === "today") return dayStart(s.now);
  if (d === "yesterday") return dayStart(s.now) - DAY;
  if (d === "tomorrow") return dayStart(s.now) + DAY;
  return parseYmd(d);
}

function previewBlocks(s: State, tp: T.TemplateView, day: number): T.PreviewBlock[] {
  return tp.blocks.map((b) => {
    const start_at = day + b.start_min * MIN;
    const end_at = day + b.end_min * MIN;
    const conflicts: T.Conflict[] = [
      ...s.blocks.filter((x) => x.end_at > start_at && x.start_at < end_at).map((x): T.Conflict => ({ kind: "block", title: x.label ?? s.tasks.find((t) => t.id === x.task_id)?.title ?? "計画ブロック", start_at: x.start_at, end_at: x.end_at })),
      ...s.calendarEvents.filter((x) => !x.all_day && x.end_at > start_at && x.start_at < end_at).map((x): T.Conflict => ({ kind: "calendar", title: x.title, start_at: x.start_at, end_at: x.end_at })),
    ];
    return { start_at, end_at, label: b.label, focus: b.focus, task_id: b.task_id, task_title: b.task_title, conflicts };
  });
}

// ===== 記録 =====

function recordText(s: State, kind: T.ErgRecordKind, id: string): { at: number; text: string } {
  if (kind === "checkin") {
    const c = s.checkins.find((x) => x.id === id);
    return { at: c?.at ?? 0, text: c?.note ?? "（メモなし）" };
  }
  if (kind === "hassle") {
    const h = s.hassles.find((x) => x.id === id);
    return { at: h?.at ?? 0, text: h?.text ?? "" };
  }
  if (kind === "abc") {
    const a = s.abcs.find((x) => x.id === id);
    return { at: a?.at ?? 0, text: a?.activating_event ?? "" };
  }
  const r = s.reflections.find((x) => x.id === id);
  return { at: r?.at ?? 0, text: r?.answers_md ?? "" };
}

function upsertErg(s: State, kind: T.ErgRecordKind, id: string, patch: Partial<T.ErgLabel>): T.ErgLabel {
  let l = s.ergLabels.find((x) => x.record_kind === kind && x.record_id === id);
  if (!l) {
    l = { record_kind: kind, record_id: id, tag: "none", source: "user", confidence: null, reason: null, at: s.now };
    s.ergLabels.push(l);
  }
  Object.assign(l, patch, { at: s.now });
  return l;
}

function items<R extends { at: number }>(s: State, list: R[], a?: T.RangeArgs): T.Items<R> {
  const [from, to] = range(s, a);
  return { start_at: from, end_at: to, items: list.filter((x) => x.at >= from && x.at < to).sort((x, y) => y.at - x.at) };
}

function structuredQuestions(kind: T.ReflectionKind): T.StructuredOutput {
  const q: T.Question[] = [
    { kind: "checkin", prompt: kind === "day_start" ? "いまの調子を教えてください。" : "今日の終わりの調子はどうですか。", scales: ["energy", "mood", "stress"] },
    { kind: "hassle", prompt: "今日、引っかかったことはありましたか。", examples: ["同じ質問が何度も来て作業が途切れた", "資料の場所が分からなかった"] },
    { kind: "abc", prompt: "気になった出来事を、そのときの考えと気持ちと一緒に書いてみませんか。", activating_event: "レビューで設計の前提を指摘された", belief_prompt: "そのとき、どんな考えが浮かびましたか。" },
    { kind: "free", prompt: "ほかに書き残しておきたいことがあれば自由にどうぞ。" },
  ];
  const summary =
    kind === "weekly"
      ? "今週は設計とレビュー対応に多くの時間が入っていました。確認待ちの区間が数件残っています。"
      : kind === "day_start"
        ? "昨日は設計書とレビュー対応が中心でした。今日は午前に見積もりの指摘対応、午後に設計書の予定です。"
        : "今日は見積もりレビューの指摘対応に最も時間が入っています。会議は朝会のみでした。";
  return { summary, questions: q };
}

function configGet(s: State, key: string): unknown {
  let cur: unknown = s.config;
  for (const p of key.split(".")) {
    if (cur && typeof cur === "object" && p in (cur as A)) cur = (cur as A)[p];
    else fail("not_found", `設定キーがありません: ${key}`);
  }
  return cur;
}

function configSet(s: State, key: string, raw: string) {
  let v: unknown;
  try {
    v = JSON.parse(raw);
  } catch {
    v = raw;
  }
  const parts = key.split(".");
  let cur = s.config as A;
  for (const p of parts.slice(0, -1)) {
    if (typeof cur[p] !== "object" || cur[p] === null) cur[p] = {};
    cur = cur[p];
  }
  cur[parts[parts.length - 1]] = v;
}

function clearCorrections(s: State, taskId: string) {
  s.corrections = s.corrections.filter((c) => c.task_id !== taskId);
}

// ===== コマンド表 =====

const H: Record<string, Handler> = {
  status: (s) => {
    const [from, to] = range(s, { date: "this-week" });
    const needs = inRange(s, from, to).filter((x) => x.status === "needs_review");
    const minMs = Number(configGet(s, "review.min_sec")) * 1000;
    const status: T.Status = {
      now: s.now,
      now_local: isoJst(s.now),
      doing: s.tasks.filter((t) => t.status === "doing").map((t) => ({ task: taskView(s, t), since: s.doingSince[t.id] ?? null })),
      recent_segments: inRange(s, s.now - 6 * HOUR, s.now).slice(-5).reverse().map((x) => segView(s, x)),
      today: todaySummary(s),
      needs_review_count: needs.filter((x) => x.end_at - x.start_at >= minMs).length,
      needs_review_short_count: needs.filter((x) => x.end_at - x.start_at < minMs).length,
      unanswered_pushes: s.pushes.filter((p) => p.state === "unanswered"),
      update_available: null,
    };
    return status;
  },

  // タスク
  task_list: (s, a) => {
    const st: T.TaskStatus[] = a.filter?.statuses ?? [];
    const kind: T.TaskKind | undefined = a.filter?.kind;
    const tasks = s.tasks.filter((t) => (st.length === 0 || st.includes(t.status)) && (!kind || t.kind === kind));
    return { tasks: tasks.map((t) => taskView(s, t)) };
  },
  task_show: (s, a) => {
    const t = needTask(s, a.id);
    return { task: taskView(s, t), rules: s.taskRules.filter((r) => r.task_id === t.id), tracked_ms: trackedMs(s, t.id), tracked_segments: s.segments.filter((x) => x.task_id === t.id).length } satisfies T.TaskShow;
  },
  task_add: (s, a) => {
    const i = a.input as T.TaskInput;
    const t: TaskRec = {
      id: nid(s, "task"),
      title: i.title ?? "（無題）",
      body_md: "",
      status: "todo",
      kind: "normal",
      urgent: null,
      important: null,
      estimate_min: null,
      planned_start_at: null,
      planned_end_at: null,
      key_result_id: null,
      external_url: null,
      created_at: s.now,
      updated_at: s.now,
      done_at: null,
      skills: [],
    };
    applyTaskInput(s, t, i);
    s.tasks.unshift(t);
    return taskView(s, t);
  },
  task_edit: (s, a) => {
    const t = needTask(s, a.id);
    applyTaskInput(s, t, a.input);
    return taskView(s, t);
  },
  task_start: (s, a) => startTask(s, needTask(s, a.id)),
  task_done: (s, a) => {
    const t = needTask(s, a.id);
    t.status = "done";
    t.done_at = s.now;
    t.updated_at = s.now;
    delete s.doingSince[t.id];
    return taskView(s, t);
  },
  task_drop: (s, a) => {
    const t = needTask(s, a.id);
    t.status = "dropped";
    t.updated_at = s.now;
    delete s.doingSince[t.id];
    return taskView(s, t);
  },
  task_delete_preview: (s, a) => deletePreview(s, needTask(s, a.id)),
  task_delete: (s, a) => {
    const t = needTask(s, a.id);
    const p = deletePreview(s, t);
    s.segments.forEach((x) => {
      if (x.task_id === t.id) x.task_id = null;
    });
    s.tasks = s.tasks.filter((x) => x.id !== t.id);
    s.taskRules = s.taskRules.filter((r) => r.task_id !== t.id);
    s.blocks = s.blocks.filter((b) => b.task_id !== t.id);
    clearCorrections(s, t.id);
    delete s.doingSince[t.id];
    return { deleted: p, range: null } satisfies T.TaskDeleteResult;
  },
  rule_add: (s, a) => {
    const i = a.input as T.RuleInput;
    const r: T.TaskMatchRule = { id: nid(s, "rule"), task_id: needTask(s, i.task).id, field: i.field, pattern: i.pattern, pattern_type: i.pattern_type ?? "contains", priority: i.priority ?? 0, enabled: true };
    s.taskRules.push(r);
    return r;
  },
  quick_previous: (s) => {
    const t = s.previousDoing ? s.tasks.find((x) => x.id === s.previousDoing && x.status !== "dropped") : undefined;
    return t ? taskView(s, t) : null;
  },
  quick_switch: (s, a) => {
    const i = a.input as { query: string; new?: boolean; interrupt?: boolean };
    let created = false;
    let t = findTask(s, i.query);
    if (!t && i.new) {
      const v = H.task_add(s, { input: { title: i.query, interrupt: i.interrupt } }, undefined as never) as T.TaskView;
      t = s.tasks.find((x) => x.id === v.id)!;
      created = true;
    }
    if (!t) fail("not_found", `タスクが見つかりません: ${i.query}`);
    return { ...startTask(s, t), created } satisfies T.SwitchResult;
  },
  quick_back: (s) => {
    const t = s.previousDoing ? s.tasks.find((x) => x.id === s.previousDoing) : undefined;
    if (!t) fail("not_found", "戻れるタスクがありません。");
    return { ...startTask(s, t), created: false } satisfies T.SwitchResult;
  },
  quick_close: () => null,
  quick_shortcut_state: (s) => ({ shortcut: s.quickShortcut, registered: s.quickShortcut || null, error: null }) satisfies T.QuickShortcutState,
  quick_shortcut_set: (s, a) => {
    s.quickShortcut = a.shortcut;
    return { shortcut: a.shortcut, registered: a.shortcut || null };
  },
  rule_list: (s, a) => ({ rules: s.taskRules.filter((r) => !a.task || r.task_id === a.task) }),

  // 区間・仕分け
  log: (s, a) => {
    const [from, to] = range(s, a.range);
    const segs = inRange(s, from, to);
    const gaps: T.GapView[] = [];
    for (let i = 1; i < segs.length; i++) {
      const g0 = segs[i - 1].end_at;
      const g1 = segs[i].start_at;
      if (g1 - g0 >= 2 * MIN) gaps.push({ start_at: g0, end_at: g1, start: isoJst(g0), end: isoJst(g1), duration_ms: g1 - g0 });
    }
    return { range: rangeView(from, to), segments: segs.map((x) => segView(s, x)), gaps } satisfies T.Log;
  },
  review_list: (s, a) => {
    const [from, to] = range(s, a.range);
    const minSec = Number(configGet(s, "review.min_sec"));
    const all = inRange(s, from, to).filter((x) => x.status === "needs_review");
    const shown = a.all ? all : all.filter((x) => x.end_at - x.start_at >= minSec * 1000);
    return { range: rangeView(from, to), count: shown.length, segments: shown.map((x) => segView(s, x)), hidden_short: all.length - shown.length, min_sec: minSec } satisfies T.ReviewList;
  },
  review_set: (s, a) => {
    const i = a.input as T.ReviewSetInput;
    const r = setSegment(s, i.segment, i);
    return { segment: segView(s, r.seg), override_id: r.overrideId, rule: r.rule } satisfies T.ReviewSetResult;
  },
  review_set_many: (s, a) => {
    const i = a.input as T.ReviewBatchInput;
    const rs = i.segments.map((id) => setSegment(s, id, i));
    return { count: rs.length, segments: rs.map((r) => segView(s, r.seg)), override_ids: rs.map((r) => r.overrideId), rules: rs.map((r) => r.rule).filter((r): r is T.CreatedRule => !!r) } satisfies T.ReviewBatchResult;
  },
  review_idle: (s, a) => {
    const segs = (a.segments as string[]).map((id) => {
      const seg = s.segments.find((x) => x.id === id);
      if (!seg) fail("not_found", `区間が見つかりません: ${id}`);
      applyIdle(seg, a.preset);
      return seg;
    });
    return { count: segs.length, segments: segs.map((x) => segView(s, x)), override_ids: segs.map(() => nid(s, "ovr")), rules: [] } satisfies T.ReviewBatchResult;
  },
  review_suggestions: (s) => s.ruleSuggestions,
  review_suggest_accept: (s, a) => {
    const i = a.input as T.RuleSuggestionRef;
    const r: T.TaskMatchRule = { id: nid(s, "rule"), task_id: i.task_id, field: i.field, pattern: i.pattern, pattern_type: i.pattern_type, priority: 0, enabled: true };
    s.taskRules.push(r);
    s.ruleSuggestions = s.ruleSuggestions.filter((x) => !(x.task_id === i.task_id && x.pattern === i.pattern));
    return { rule_type: "task_match_rule", id: r.id, field: r.field, pattern: r.pattern, pattern_type: r.pattern_type, task_id: r.task_id, activity_label: null, reused: false } satisfies T.CreatedRule;
  },
  review_suggest_dismiss: (s, a) => {
    const i = a.input as T.RuleSuggestionRef;
    s.ruleSuggestions = s.ruleSuggestions.filter((x) => !(x.task_id === i.task_id && x.pattern === i.pattern));
    return null;
  },
  review_ignore: (s, a) => {
    const segs = (a.segments as string[]).map((id) => {
      const seg = s.segments.find((x) => x.id === id);
      if (!seg) fail("not_found", `区間が見つかりません: ${id}`);
      seg.kind = "ignored";
      seg.status = "confirmed";
      seg.decided_by = "user";
      return seg;
    });
    return { count: segs.length, segments: segs.map((x) => segView(s, x)), override_ids: [], rules: [] } satisfies T.ReviewBatchResult;
  },
  review_unignore: (s, a) => {
    const ids = a.segments as string[];
    for (const x of s.segments) if (ids.includes(x.id)) Object.assign(x, { kind: "work", status: "needs_review", decided_by: "heuristic" });
    return { count: ids.length, overrides_removed: ids.length, range: rangeView(dayStart(s.now), dayStart(s.now) + DAY) } satisfies T.UnignoreResult;
  },
  ignore_list: (s) => ({ exes: s.ignoreExes, default_exes: DEFAULT_IGNORE_EXES, customized: s.ignoreCustomized }) satisfies T.IgnoreList,
  ignore_add: (s, a) => {
    const changed = !s.ignoreExes.includes(a.exe);
    if (changed) s.ignoreExes.push(a.exe);
    s.ignoreCustomized = true;
    s.ignoreSuggestions = s.ignoreSuggestions.filter((x) => x.exe !== a.exe);
    return { exe: a.exe, changed, exes: s.ignoreExes, recomputed: null } satisfies T.IgnoreChange;
  },
  ignore_remove: (s, a) => {
    const changed = s.ignoreExes.includes(a.exe);
    s.ignoreExes = s.ignoreExes.filter((x) => x !== a.exe);
    s.ignoreCustomized = true;
    return { exe: a.exe, changed, exes: s.ignoreExes, recomputed: null } satisfies T.IgnoreChange;
  },
  ignore_suggestions: (s) => s.ignoreSuggestions,
  ignore_suggest_dismiss: (s, a) => {
    s.ignoreSuggestions = s.ignoreSuggestions.filter((x) => x.exe !== a.exe);
    return null;
  },

  // 実績の補正
  correction_candidates: (s, a) => ({ range: rangeView(a.start_at, a.end_at), snap_sec: Number(configGet(s, "review.snap_sec")), candidates: candidatesFor(s, a.start_at, a.end_at) }) satisfies T.CorrectionCandidates,
  correction_preview: (s, a) => correctionPreview(s, a.input),
  correction_apply: (s, a) => {
    const i = a.input as T.CorrectionInput;
    const preview = correctionPreview(s, i);
    const before = inRange(s, i.start_at, i.end_at).map((x) => ({ ...x }));
    const task = i.task ? needTask(s, i.task) : undefined;
    let n = 0;
    for (const c of preview.candidates) {
      if (!c.checked) continue;
      let seg: SegRec;
      if (c.gap) {
        seg = { id: nid(s, "seg"), start_at: c.start_at, end_at: c.end_at, kind: "work", task_id: null, activity_label: null, confidence: 1, decided_by: "user", status: "confirmed", agent_session_id: null, exe_name: null, title: null };
        s.segments.push(seg);
      } else {
        const orig = s.segments.find((x) => x.id === c.id);
        if (!orig) continue;
        seg = splitSegment(s, orig, c.start_at, c.end_at);
      }
      seg.task_id = i.rest || i.no_task ? null : task?.id ?? seg.task_id;
      seg.kind = i.rest ? "break" : seg.kind === "idle" || seg.kind === "break" ? "work" : seg.kind;
      if (i.activity) seg.activity_label = i.activity;
      seg.offline = c.becomes_offline && !i.rest;
      seg.status = "confirmed";
      seg.decided_by = "user";
      n += 1;
    }
    const rec: CorrectionRec = { id: nid(s, "corr"), start_at: i.start_at, end_at: i.end_at, task_id: task?.id ?? null, kind: i.rest ? "break" : null, activity_label: i.activity ?? null, total_ms: preview.total_ms, offline_ms: preview.offline_ms, segment_count: n, created_at: s.now, before };
    s.corrections.unshift(rec);
    return { correction: correctionView(s, rec), preview, override_ids: [] } satisfies T.CorrectionApplied;
  },
  correction_list: (s, a) => {
    let list = s.corrections;
    if (a.filter?.task) list = list.filter((c) => c.task_id === a.filter.task);
    if (a.filter?.date) {
      const d = parseYmd(a.filter.date);
      list = list.filter((c) => c.start_at < d + DAY && c.end_at > d);
    }
    return { count: list.length, corrections: list.map((c) => correctionView(s, c)) } satisfies T.CorrectionList;
  },
  correction_undo: (s, a) => {
    const c = s.corrections.find((x) => x.id === a.id);
    if (!c) fail("not_found", "補正が見つかりません。");
    s.segments = s.segments.filter((x) => !(x.end_at > c.start_at && x.start_at < c.end_at));
    s.segments.push(...c.before.map((x) => ({ ...x })));
    s.corrections = s.corrections.filter((x) => x.id !== c.id);
    return { correction: correctionView(s, c), overrides_removed: c.segment_count, range: rangeView(c.start_at, c.end_at) } satisfies T.CorrectionUndone;
  },

  // 計画
  block_add: (s, a) => {
    const b = blockFromInput(s, a.input, nid(s, "blk"));
    s.blocks.push(b);
    return b;
  },
  block_list: (s, a) => {
    const [from, to] = range(s, a.range);
    return { range: rangeView(from, to), blocks: s.blocks.filter((b) => b.end_at > from && b.start_at < to).sort((x, y) => x.start_at - y.start_at).map((b) => blockView(s, b)) } satisfies T.BlockList;
  },
  block_update: (s, a) => {
    const i = s.blocks.findIndex((b) => b.id === a.id);
    if (i < 0) fail("not_found", "ブロックが見つかりません。");
    s.blocks[i] = blockFromInput(s, a.input, a.id);
    return s.blocks[i];
  },
  block_delete: (s, a) => {
    s.blocks = s.blocks.filter((b) => b.id !== a.id);
    return null;
  },
  block_template_list: (s) => ({ templates: s.templates }),
  block_template_add: (s, a) => {
    const t = templateFromInput(s, a.input, nid(s, "tpl"), s.now);
    s.templates.push(t);
    return t;
  },
  block_template_edit: (s, a) => {
    const i = s.templates.findIndex((t) => t.id === a.id);
    if (i < 0) fail("not_found", "テンプレートが見つかりません。");
    s.templates[i] = templateFromInput(s, a.input, a.id, s.templates[i].created_at);
    return s.templates[i];
  },
  block_template_delete: (s, a) => {
    const t = needTemplate(s, a.id);
    s.templates = s.templates.filter((x) => x.id !== t.id);
    return t;
  },
  block_template_suggest: (s, a) => {
    const day = dateOf(s, a.date);
    const weekday = weekdayMon0(day) + 1;
    return { date: ymd(day), weekday, templates: s.templates.filter((t) => t.weekdays.includes(weekday)) } satisfies T.Suggestion;
  },
  block_template_preview: (s, a) => {
    const i = a.input as T.ApplyInput;
    const tp = needTemplate(s, i.template);
    const day = dateOf(s, i.date);
    return { template: tp.name, date: ymd(day), overlap: i.overlap ?? "skip", blocks: previewBlocks(s, tp, day) } satisfies T.TemplatePreview;
  },
  block_template_apply: (s, a) => {
    const i = a.input as T.ApplyInput;
    const tp = needTemplate(s, i.template);
    const day = dateOf(s, i.date);
    const overlay = i.overlap === "overlay";
    const created: T.BlockView[] = [];
    const skipped: T.PreviewBlock[] = [];
    for (const p of previewBlocks(s, tp, day)) {
      if (!overlay && p.conflicts.length > 0) {
        skipped.push(p);
        continue;
      }
      const b: T.TimeBlock = { id: nid(s, "blk"), start_at: p.start_at, end_at: p.end_at, task_id: p.task_id, label: p.label, focus: p.focus };
      s.blocks.push(b);
      created.push(blockView(s, b));
    }
    const appId = nid(s, "app");
    s.appliedTemplates[ymd(day)] = created.map((b) => b.id);
    return { application_id: appId, template: tp.name, date: ymd(day), created, skipped } satisfies T.ApplyResult;
  },
  block_template_undo: (s, a) => {
    const day = ymd(dateOf(s, a.date));
    const ids = s.appliedTemplates[day] ?? [];
    const before = s.blocks.length;
    s.blocks = s.blocks.filter((b) => !ids.includes(b.id));
    delete s.appliedTemplates[day];
    return { template: s.templates[0]?.name ?? "", date: day, removed: before - s.blocks.length } satisfies T.UndoResult;
  },
  block_template_save_from: (s, a) => {
    const i = a.input as { name: string; description?: string; date?: string; weekdays?: number[] };
    const day = dateOf(s, i.date);
    const bs = s.blocks.filter((b) => b.start_at >= day && b.start_at < day + DAY).sort((x, y) => x.start_at - y.start_at);
    const t = templateFromInput(
      s,
      {
        name: i.name,
        description: i.description,
        weekdays: i.weekdays ?? [weekdayMon0(day) + 1],
        items: bs.map((b) => ({ start: hhmm(Math.round((b.start_at - day) / MIN)), end: hhmm(Math.round((b.end_at - day) / MIN)), label: b.label, focus: b.focus, task: b.task_id })),
      },
      nid(s, "tpl"),
      s.now,
    );
    s.templates.push(t);
    return t;
  },
  hint_dismissed: (s, a) => s.hints.has(a.name),
  hint_dismiss: (s, a) => {
    s.hints.add(a.name);
    return null;
  },
  calendar_list: (s, a) => {
    const [from, to] = range(s, a.range);
    return s.calendarEvents.filter((e) => e.end_at > from && e.start_at < to);
  },

  // 記録
  checkin_add: (s, a) => {
    const i = a.input as T.CheckInput;
    const c: T.Checkin = { id: nid(s, "chk"), at: s.now, energy: i.energy ?? null, mood: i.mood ?? null, stress: i.stress ?? null, erg_tag: null, note: i.note ?? null, push_id: null };
    s.checkins.unshift(c);
    return c;
  },
  checkin_list: (s, a) => items(s, s.checkins, a.range),
  hassle_add: (s, a) => {
    const h: T.Hassle = { id: nid(s, "has"), at: s.now, text: a.input.text, intensity: a.input.intensity ?? 3, segment_id: null, push_id: null };
    s.hassles.unshift(h);
    return h;
  },
  hassle_list: (s, a) => items(s, s.hassles, a.range),
  abc_add: (s, a) => {
    const i = a.input as T.AbcInput;
    const h = i.from_hassle ? s.hassles.find((x) => x.id === i.from_hassle) : undefined;
    const r: T.AbcRecord = {
      id: nid(s, "abc"),
      at: s.now,
      activating_event: i.activating_event ?? h?.text ?? "",
      belief: i.belief ?? null,
      consequence_emotion: i.consequence_emotion ?? null,
      consequence_intensity: i.consequence_intensity ?? null,
      dispute: null,
      ai_dialog_md: null,
      hassle_id: h?.id ?? null,
    };
    s.abcs.unshift(r);
    return r;
  },
  abc_list: (s, a) => items(s, s.abcs, a.range),

  // 振り返り
  context: (s, a) => ({ kind: a.kind, date: a.date ?? ymd(s.now), tasks: s.tasks.length }),
  reflect_generate: (s, a) => {
    const so = structuredQuestions(a.kind);
    const r: T.Reflection = {
      id: nid(s, "ref"),
      at: s.now,
      kind: a.kind,
      context_json: "{}",
      questions_md: so.questions.map((q) => `- ${q.prompt}`).join("\n"),
      answers_md: null,
      summary_md: `## ${so.summary}`,
      structured_json: JSON.stringify(so),
      answers_json: null,
    };
    s.reflections.unshift(r);
    return r;
  },
  reflect_answer: (s, a) => {
    const r = s.reflections.find((x) => x.id === a.id);
    if (!r) fail("not_found", "振り返りが見つかりません。");
    r.answers_md = a.answers_md;
    return r;
  },
  reflect_answer_structured: (s, a) => {
    const r = s.reflections.find((x) => x.id === a.id);
    if (!r) fail("not_found", "振り返りが見つかりません。");
    const so = JSON.parse(r.structured_json ?? "{}") as T.StructuredOutput;
    const out: T.AnsweredItem[] = [];
    for (const it of (a.answers as T.AnswersInput).answers) {
      const q = so.questions?.[it.index];
      if (!q) continue;
      if (q.kind === "checkin") {
        const c = H.checkin_add(s, { input: { energy: it.energy, mood: it.mood, stress: it.stress, note: it.note } }, undefined as never) as T.Checkin;
        out.push({ index: it.index, kind: q.kind, record_id: c.id });
      } else if (q.kind === "hassle") {
        const h = H.hassle_add(s, { input: { text: it.text ?? "", intensity: it.intensity } }, undefined as never) as T.Hassle;
        out.push({ index: it.index, kind: q.kind, record_id: h.id });
      } else if (q.kind === "abc") {
        const b = H.abc_add(s, { input: { activating_event: it.activating_event, belief: it.belief, consequence_emotion: it.emotion, consequence_intensity: it.emotion_intensity } }, undefined as never) as T.AbcRecord;
        out.push({ index: it.index, kind: q.kind, record_id: b.id });
      } else {
        out.push({ index: it.index, kind: q.kind, text: it.text });
      }
    }
    r.answers_json = JSON.stringify({ items: out } satisfies T.AnswersRecord);
    return r;
  },
  reflect_answer_details: (s, a) => {
    const r = s.reflections.find((x) => x.id === a.id);
    if (!r?.answers_json) return [];
    const so = JSON.parse(r.structured_json ?? "{}") as T.StructuredOutput;
    return (JSON.parse(r.answers_json) as T.AnswersRecord).items.map((it): T.AnswerDetail => ({
      index: it.index,
      kind: it.kind,
      prompt: so.questions?.[it.index]?.prompt ?? "",
      record_id: it.record_id,
      text: it.text,
      checkin: s.checkins.find((x) => x.id === it.record_id),
      hassle: s.hassles.find((x) => x.id === it.record_id),
      abc: s.abcs.find((x) => x.id === it.record_id),
      missing: it.record_id ? ![...s.checkins, ...s.hassles, ...s.abcs].some((x) => x.id === it.record_id) : undefined,
    }));
  },
  reflect_list: (s, a) => s.reflections.filter((r) => !a.kind || r.kind === a.kind).slice(0, a.limit ?? 30),
  reflect_show: (s, a) => {
    const r = s.reflections.find((x) => x.id === a.id);
    if (!r) fail("not_found", "振り返りが見つかりません。");
    return r;
  },
  abc_dispute: (s, a) => {
    const r = s.abcs.find((x) => x.id === a.id);
    if (!r) fail("not_found", "捉え直しが見つかりません。");
    r.dispute = "その考えを裏づける事実と、そうでない事実を並べてみると、どう見えますか。";
    r.ai_dialog_md = "**問い**: 指摘された点は設計全体への評価でしたか、それとも前提の一部についてでしたか。";
    return r;
  },

  // ERG
  erg_overview: (s, a) => {
    const [from, to] = range(s, a.range);
    const labels = s.ergLabels.filter((l) => l.at >= from && l.at < to);
    const count = (tag: T.ErgChoice) => labels.filter((l) => l.source !== "suggested" && l.tag === tag).length;
    const total = [...s.checkins, ...s.hassles, ...s.abcs, ...s.reflections].filter((x) => x.at >= from && x.at < to).length;
    const labeled = labels.length;
    return {
      range: rangeView(from, to),
      distribution: {
        existence: count("existence"),
        relatedness: count("relatedness"),
        growth: count("growth"),
        none: count("none"),
        pending: labels.filter((l) => l.source === "suggested").length,
        unlabeled: Math.max(0, total - labeled),
        ai_decided: labels.filter((l) => l.source === "ai").length,
        total,
        legend: "記録の内容がどの観点に関わるかの件数です。多い少ないに良し悪しはありません。",
      },
      labels,
      pending_total: s.ergLabels.filter((l) => l.source === "suggested").length,
    } satisfies T.ErgOverview;
  },
  erg_review: (s) => ({
    items: s.ergLabels
      .filter((l) => l.source === "suggested")
      .map((l): T.ErgItem => {
        const r = recordText(s, l.record_kind, l.record_id);
        return { ...l, record_at: r.at, excerpt: r.text };
      }),
  }),
  erg_classify: (s, a) => {
    const [from, to] = range(s, a.range);
    return { range: rangeView(from, to), dry_run: false, skipped: "判定器が設定されていないため、判定しませんでした。", targets: 0, adopted: 0, pending: 0, unread: 0, items: [] } satisfies T.ErgClassifyOutcome;
  },
  erg_set: (s, a) => upsertErg(s, a.record_kind, a.record_id, { tag: a.tag, source: "user", confidence: null, reason: null }),
  erg_confirm: (s, a) => upsertErg(s, a.record_kind, a.record_id, { source: "user" }),

  // OKR・スキル
  okr_list: (s) => ({ objectives: s.objectives.map((o) => ({ ...o, key_results: s.krs.filter((k) => k.objective_id === o.id) })) }),
  okr_add_objective: (s, a) => {
    const o: T.Objective = { id: nid(s, "obj"), title: a.input.title, description_md: null, period_start: null, period_end: null, status: "active" };
    s.objectives.push(o);
    return o;
  },
  okr_add_kr: (s, a) => {
    const i = a.input as { objective: string; title: string; target?: string; skills?: string[] };
    const o = s.objectives.find((x) => x.id === i.objective || x.title === i.objective);
    if (!o) fail("not_found", "目標が見つかりません。");
    const k: T.KeyResult = { id: nid(s, "kr"), objective_id: o.id, title: i.title, target: i.target ?? null, current: null, status: "active", measure: "time", day_min: 30 };
    s.krs.push(k);
    if (i.skills?.length) s.krSkills[k.id] = i.skills;
    return k;
  },
  skill_list: (s) => ({ skills: s.skills.map((k) => ({ ...k, parent_name: s.skills.find((p) => p.id === k.parent_id)?.name ?? null })) }),
  skill_add: (s, a) => {
    const p = a.input.parent ? s.skills.find((x) => x.name === a.input.parent || x.id === a.input.parent) : undefined;
    const k: T.Skill = { id: nid(s, "skill"), name: a.input.name, parent_id: p?.id ?? null, description: null, preset: false };
    s.skills.push(k);
    return k;
  },
  skill_import_preset: (s, a) => {
    const names = ["要件定義", "テスト設計", "ドキュメンテーション"];
    let added = 0;
    for (const n of names) {
      if (!s.skills.some((x) => x.name === n)) {
        s.skills.push({ id: nid(s, "skill"), name: n, parent_id: null, description: null, preset: true });
        added += 1;
      }
    }
    return { preset: a.name ?? "default", added, skipped: names.length - added };
  },
  okr_edit_objective: (s, a) => {
    const o = s.objectives.find((x) => x.id === a.id);
    if (!o) fail("not_found", "目標が見つかりません。");
    const i = a.input as T.ObjectiveEdit;
    if (i.title !== undefined) o.title = i.title;
    if (i.description !== undefined) o.description_md = i.description || null;
    if (i.from !== undefined) o.period_start = i.from;
    if (i.to !== undefined) o.period_end = i.to;
    if (i.clear_period) o.period_start = o.period_end = null;
    if (i.status !== undefined) o.status = i.status;
    return o;
  },
  okr_delete_objective: (s, a) => {
    const o = s.objectives.find((x) => x.id === a.id);
    if (!o) fail("not_found", "目標が見つかりません。");
    const krs = s.krs.filter((k) => k.objective_id === o.id);
    return deleteOutcome("objective", o.id, o.title, [{ label: "KR", count: krs.length }, { label: "紐付けたタスク", count: s.tasks.filter((t) => krs.some((k) => k.id === t.key_result_id)).length }], a.mode, () => {
      s.tasks.forEach((t) => {
        if (krs.some((k) => k.id === t.key_result_id)) t.key_result_id = null;
      });
      s.krs = s.krs.filter((k) => k.objective_id !== o.id);
      s.objectives = s.objectives.filter((x) => x.id !== o.id);
    });
  },
  okr_edit_kr: (s, a) => {
    const k = needKr(s, a.id);
    const i = a.input as T.KrEdit;
    if (i.title !== undefined) k.title = i.title;
    if (i.target !== undefined) k.target = i.target || null;
    if (i.current !== undefined && i.current !== (k.current ?? "")) {
      k.current = i.current || null;
      s.krProgress.push({ id: nid(s, "prog"), kr_id: k.id, value: i.current ? numberOf(i.current) : null, text: i.current || null, note: i.current_note ?? null, at: s.now });
    }
    if (i.status !== undefined) k.status = i.status;
    if (i.objective) k.objective_id = s.objectives.find((o) => o.id === i.objective || o.title === i.objective)?.id ?? k.objective_id;
    if (i.measure) k.measure = i.measure;
    if (i.day_min !== undefined) k.day_min = i.day_min;
    if (i.skills?.length) s.krSkills[k.id] = i.skills;
    if (i.clear_skills) delete s.krSkills[k.id];
    return k;
  },
  okr_delete_kr: (s, a) => {
    const k = needKr(s, a.id);
    return deleteOutcome("key_result", k.id, k.title, [{ label: "紐付けたタスク", count: s.tasks.filter((t) => t.key_result_id === k.id).length }, { label: "現在値の履歴", count: s.krProgress.filter((p) => p.kr_id === k.id).length }], a.mode, () => {
      s.tasks.forEach((t) => {
        if (t.key_result_id === k.id) t.key_result_id = null;
      });
      s.krs = s.krs.filter((x) => x.id !== k.id);
      s.krProgress = s.krProgress.filter((p) => p.kr_id !== k.id);
    });
  },
  okr_link: (s, a) => {
    const k = needKr(s, a.kr);
    const linked = (a.tasks as string[]).map((q): T.LinkedTask => {
      const t = needTask(s, q);
      const prev = t.key_result_id ? s.krs.find((x) => x.id === t.key_result_id) : undefined;
      t.key_result_id = k.id;
      return { id: t.id, title: t.title, previous_kr: prev ? { id: prev.id, title: prev.title } : null };
    });
    return { key_result: k, linked } satisfies T.LinkResult;
  },
  okr_unlink: (s, a) =>
    (a.tasks as string[]).map((q): T.LinkedTask => {
      const t = needTask(s, q);
      const prev = t.key_result_id ? s.krs.find((x) => x.id === t.key_result_id) : undefined;
      t.key_result_id = null;
      return { id: t.id, title: t.title, previous_kr: prev ? { id: prev.id, title: prev.title } : null };
    }),
  okr_show: (s, a) => krSummary(s, needKr(s, a.kr)),
  okr_suggest: (s, a) => {
    const k = needKr(s, a.kr);
    const eligible = !(s.krSkills[k.id]?.length) && !s.tasks.some((t) => t.key_result_id === k.id);
    const candidates: T.KrCandidate[] = eligible
      ? [
          ...s.tasks.filter((t) => !t.key_result_id && t.status !== "dropped").slice(0, 3).map((t, n): T.KrCandidate => ({ kind: "task", id: t.id, name: t.title, score: 0.8 - n * 0.1, source: "local" })),
          ...s.skills.slice(0, 2).map((x, n): T.KrCandidate => ({ kind: "skill", id: x.id, name: x.name, score: 0.7 - n * 0.1, source: "local" })),
        ]
      : [];
    return { key_result: { id: k.id, title: k.title }, eligible, candidates, ai: { used: false, note: a.use_ai ? "AI は設定されていないため、手元の候補だけを出しました。" : null } } satisfies T.KrSuggestions;
  },
  okr_suggest_accept: (s, a) => {
    const k = needKr(s, a.kr);
    const out: T.KrSuggestDecision["items"] = [];
    for (const id of a.candidates as string[]) {
      const t = s.tasks.find((x) => x.id === id);
      const sk = s.skills.find((x) => x.id === id);
      if (t) {
        t.key_result_id = k.id;
        out.push({ kind: "task", id, name: t.title });
      } else if (sk) {
        s.krSkills[k.id] = [...(s.krSkills[k.id] ?? []), sk.name];
        out.push({ kind: "skill", id, name: sk.name });
      }
    }
    return { key_result: { id: k.id, title: k.title }, items: out } satisfies T.KrSuggestDecision;
  },
  okr_suggest_reject: (s, a) => {
    const k = needKr(s, a.kr);
    return { key_result: { id: k.id, title: k.title }, items: (a.candidates as string[]).map((id) => ({ kind: "task" as const, id, name: s.tasks.find((t) => t.id === id)?.title ?? id })) } satisfies T.KrSuggestDecision;
  },
  okr_progress: (s, a) => {
    const k = needKr(s, a.kr);
    const e: T.KrProgressEntry = { id: nid(s, "prog"), kr_id: k.id, value: numberOf(a.value), text: a.value || null, note: a.note ?? null, at: s.now };
    s.krProgress.push(e);
    k.current = a.value || null;
    return { key_result: k, entry: e } satisfies T.ProgressRecorded;
  },
  okr_history: (s, a) => krHistory(s, needKr(s, a.kr)),
  skill_edit: (s, a) => {
    const k = s.skills.find((x) => x.id === a.key || x.name === a.key);
    if (!k) fail("not_found", "スキルが見つかりません。");
    const i = a.input as T.SkillEdit;
    const old = k.name;
    if (i.name) {
      k.name = i.name;
      s.tasks.forEach((t) => (t.skills = t.skills.map((n) => (n === old ? i.name! : n))));
    }
    if (i.parent) k.parent_id = s.skills.find((x) => x.name === i.parent || x.id === i.parent)?.id ?? k.parent_id;
    if (i.clear_parent) k.parent_id = null;
    if (i.description !== undefined) k.description = i.description || null;
    return k;
  },
  skill_delete: (s, a) => {
    const k = s.skills.find((x) => x.id === a.key || x.name === a.key);
    if (!k) fail("not_found", "スキルが見つかりません。");
    return deleteOutcome("skill", k.id, k.name, [{ label: "付いているタスク", count: s.tasks.filter((t) => t.skills.includes(k.name)).length }, { label: "子スキル", count: s.skills.filter((x) => x.parent_id === k.id).length }], a.mode, () => {
      s.tasks.forEach((t) => (t.skills = t.skills.filter((n) => n !== k.name)));
      s.skills.forEach((x) => {
        if (x.parent_id === k.id) x.parent_id = a.children === "reparent" ? k.parent_id : x.parent_id;
      });
      if (a.children === "delete") s.skills = s.skills.filter((x) => x.parent_id !== k.id);
      s.skills = s.skills.filter((x) => x.id !== k.id);
    });
  },

  // ルール
  rule_edit: (s, a) => {
    const r = s.taskRules.find((x) => x.id === a.id);
    if (!r) fail("not_found", "ルールが見つかりません。");
    Object.assign(r, a.input);
    return r;
  },
  rule_delete: (s, a) => {
    s.taskRules = s.taskRules.filter((x) => x.id !== a.id);
    return { deleted: true };
  },
  activity_rule_list: (s) => ({ rules: s.activityRules }),
  activity_rule_add: (s, a) => {
    const i = a.input as T.ActivityRuleInput;
    const r: T.ActivityRule = { id: nid(s, "arule"), field: i.field, pattern: i.pattern, pattern_type: i.pattern_type ?? "contains", activity_label: i.activity_label, priority: i.priority ?? 0, enabled: true };
    s.activityRules.push(r);
    return r;
  },
  activity_rule_edit: (s, a) => {
    const r = s.activityRules.find((x) => x.id === a.id);
    if (!r) fail("not_found", "ルールが見つかりません。");
    Object.assign(r, a.input);
    return r;
  },
  activity_rule_delete: (s, a) => {
    s.activityRules = s.activityRules.filter((x) => x.id !== a.id);
    return { deleted: true };
  },

  // 記録の編集と削除
  checkin_edit: (s, a) => {
    const c = s.checkins.find((x) => x.id === a.id);
    if (!c) fail("not_found", "調子が見つかりません。");
    const i = a.input as T.CheckInput;
    Object.assign(c, { energy: i.energy ?? null, mood: i.mood ?? null, stress: i.stress ?? null, note: i.note ?? null });
    return c;
  },
  checkin_delete: (s, a) => {
    const c = s.checkins.find((x) => x.id === a.id);
    if (!c) fail("not_found", "調子が見つかりません。");
    return deleteOutcome("checkin", c.id, "調子", [], a.mode, () => (s.checkins = s.checkins.filter((x) => x.id !== c.id)));
  },
  hassle_edit: (s, a) => {
    const h = s.hassles.find((x) => x.id === a.id);
    if (!h) fail("not_found", "気になったことが見つかりません。");
    h.text = a.input.text;
    if (a.input.intensity !== undefined) h.intensity = a.input.intensity;
    return h;
  },
  hassle_delete: (s, a) => {
    const h = s.hassles.find((x) => x.id === a.id);
    if (!h) fail("not_found", "気になったことが見つかりません。");
    return deleteOutcome("hassle", h.id, h.text, [{ label: "つながる捉え直し", count: s.abcs.filter((x) => x.hassle_id === h.id).length }], a.mode, () => (s.hassles = s.hassles.filter((x) => x.id !== h.id)));
  },
  abc_edit: (s, a) => {
    const r = s.abcs.find((x) => x.id === a.id);
    if (!r) fail("not_found", "捉え直しが見つかりません。");
    const i = a.input as T.AbcEditInput;
    Object.assign(r, { activating_event: i.activating_event, belief: i.belief ?? null, consequence_emotion: i.consequence_emotion ?? null, consequence_intensity: i.consequence_intensity ?? null, dispute: i.dispute ?? null });
    return r;
  },
  abc_delete: (s, a) => {
    const r = s.abcs.find((x) => x.id === a.id);
    if (!r) fail("not_found", "捉え直しが見つかりません。");
    return deleteOutcome("abc", r.id, r.activating_event, [], a.mode, () => (s.abcs = s.abcs.filter((x) => x.id !== r.id)));
  },

  // カレンダーソース・活動ラベル
  calendar_source_list: (s) => ({ sources: s.calendarSources }),
  calendar_source_add: (s, a) => {
    let host = "calendar.example.com";
    try {
      host = new URL(a.url).host;
    } catch {
      /* URL として読めなくても動く */
    }
    const v: T.CalendarSourceView = { name: a.name, type: "ics", host };
    s.calendarSources.push(v);
    return v;
  },
  calendar_source_edit: (s, a) => {
    const v = s.calendarSources.find((x) => x.name === a.name);
    if (!v) fail("not_found", "カレンダーが見つかりません。");
    if (a.new_name) v.name = a.new_name;
    return v;
  },
  calendar_source_delete: (s, a) => {
    const v = s.calendarSources.find((x) => x.name === a.name);
    if (!v) fail("not_found", "カレンダーが見つかりません。");
    return deleteOutcome("calendar_source", v.name, v.name, [{ label: "取り込み済みの予定", count: s.calendarEvents.filter((e) => e.source === v.name).length }], a.mode, () => {
      s.calendarSources = s.calendarSources.filter((x) => x.name !== v.name);
      s.calendarEvents = s.calendarEvents.filter((e) => e.source !== v.name);
    });
  },
  activity_label_list: (s) => ({ labels: s.activityLabels }),
  activity_label_add: (s, a) => {
    if (!s.activityLabels.includes(a.label)) s.activityLabels.push(a.label);
    return { labels: s.activityLabels };
  },
  activity_label_rename: (s, a) => {
    s.activityLabels = s.activityLabels.map((l) => (l === a.from ? a.to : l));
    s.segments.forEach((x) => {
      if (x.activity_label === a.from) x.activity_label = a.to;
    });
    return { labels: s.activityLabels };
  },
  activity_label_remove: (s, a) => {
    s.activityLabels = s.activityLabels.filter((l) => l !== a.label);
    return { labels: s.activityLabels };
  },

  // レポート
  report: (s, a) => buildReport(s, a.by, a.range, !!a.include_idle),
  heatmap: (s, a) => buildHeatmap(s, a.by, a.from, a.to),

  // 設定
  config_all: (s) => structuredClone(s.config),
  config_get: (s, a) => ({ key: a.key, value: configGet(s, a.key) }),
  config_set: (s, a) => {
    configSet(s, a.key, a.value);
    return { key: a.key, path: "C:\\Users\\sample\\AppData\\Roaming\\Hitoiki\\config.toml" };
  },
  config_path: () => ({ path: "C:\\Users\\sample\\AppData\\Roaming\\Hitoiki\\config.toml", db: "C:\\Users\\sample\\AppData\\Roaming\\Hitoiki\\hitoiki.db" }),

  // ウインドウ・通知
  open_screen: (_s, a, ctx) => {
    ctx.emit("open-screen", { screen: a.screen });
    return null;
  },
  snooze: (s, a) => {
    s.snoozeUntil = a.minutes > 0 ? s.now + a.minutes * MIN : 0;
    return { snooze_until: s.snoozeUntil };
  },
  snooze_state: (s) => ({ snooze_until: s.snoozeUntil }),
  take_initial_screen: () => null,
  pending_push: (s) => s.pendingPush,
  push_respond: (s, a) => {
    const p = s.pushes.find((x) => x.id === a.push_id);
    if (p) p.state = a.action === "dismiss" ? "dismissed" : "answered";
    if (a.action === "energy" && a.value) {
      s.checkins.unshift({ id: nid(s, "chk"), at: s.now, energy: a.value, mood: null, stress: null, erg_tag: null, note: a.input || null, push_id: a.push_id ?? null });
    }
    if (a.input && a.action !== "energy") s.hassles.unshift({ id: nid(s, "has"), at: s.now, text: a.input, intensity: 3, segment_id: null, push_id: a.push_id ?? null });
    s.pendingPush = null;
    return null;
  },
  runtime_state: () => ({ mode: "own" }) satisfies T.RuntimeMode,
  notify_test: () => null,

  // オンボーディング
  onboarding_status: (s) => s.onboarding,
  onboarding_complete: (s) => {
    s.onboarding = { completed: true, completed_at: s.now };
    return s.onboarding;
  },
  onboarding_reset: (s) => {
    s.onboarding = { completed: false, completed_at: null };
    return s.onboarding;
  },
};

/** 状態を変えるコマンド（実行後に再取得用のイベントを出す）。 */
const MUTATING = /_(add|edit|delete|set|set_many|start|done|drop|apply|undo|link|unlink|accept|reject|dismiss|ignore|unignore|progress|respond|answer|answer_structured|import_preset|switch|back|complete|reset|idle|save_from|update|rename|remove|generate)$/;

export interface Mock {
  state: State;
  /** コマンドを実行する。未実装なら `console.error` を出して reject する。 */
  call: (cmd: string, args?: A) => Promise<unknown>;
  commands: string[];
}

export function createMock(scenario: Scenario, ctx: Ctx = { emit: () => {} }): Mock {
  const state = buildState(scenario);
  const call = async (cmd: string, args: A = {}) => {
    // Tauri のプラグイン（ウインドウ操作など）は何もせず成功させる。
    if (cmd.startsWith("plugin:")) return null;
    const h = H[cmd];
    if (!h) {
      console.error(`[mock] unhandled command: ${cmd}`);
      throw { code: "internal", message: `[mock] unhandled command: ${cmd}` };
    }
    const out = h(state, args, ctx);
    if (MUTATING.test(cmd)) {
      ctx.emit("segments-updated");
      if (/task|quick/.test(cmd)) ctx.emit("doing-changed");
    }
    // 呼び出し側が結果を書き換えても状態が壊れないよう、複製して返す。
    return out === undefined ? null : structuredClone(out);
  };
  return { state, call, commands: Object.keys(H) };
}

export { BASE_NOW, at };
export type { Scenario };

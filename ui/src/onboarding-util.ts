// オンボーディングのステップ定義と、ステップ移動の純粋なロジック。

export const ONBOARDING_STEPS = [
  { id: "welcome", label: "ようこそ" },
  { id: "task", label: "タスク" },
  { id: "startup", label: "自動起動と通知" },
  { id: "calendar", label: "カレンダー" },
  { id: "daily", label: "日々の使い方" },
] as const;

export type OnboardingStepId = (typeof ONBOARDING_STEPS)[number]["id"];

/** 範囲外にならないようにステップ番号を丸める。 */
export function clampStep(i: number): number {
  return Math.min(Math.max(0, Math.trunc(i)), ONBOARDING_STEPS.length - 1);
}

export function isLastStep(i: number): boolean {
  return clampStep(i) === ONBOARDING_STEPS.length - 1;
}

/** 「今日」画面の空状態で案内を出すか（タスクも記録も無いとき）。 */
export function showStartGuide(taskCount: number, segmentCount: number): boolean {
  return taskCount === 0 || segmentCount === 0;
}

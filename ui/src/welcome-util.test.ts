import { describe, expect, it } from "vitest";
import { WELCOME_STEPS, clampStep, isLastStep, showStartGuide } from "./welcome-util";

describe("はじめにのステップ", () => {
  it("範囲外は端に丸める", () => {
    expect(clampStep(-3)).toBe(0);
    expect(clampStep(2)).toBe(2);
    expect(clampStep(99)).toBe(WELCOME_STEPS.length - 1);
  });

  it("最後のステップを判定する", () => {
    expect(isLastStep(0)).toBe(false);
    expect(isLastStep(WELCOME_STEPS.length - 1)).toBe(true);
  });

  it("今日画面の案内はタスクか記録が無いときに出す", () => {
    expect(showStartGuide(0, 0)).toBe(true);
    expect(showStartGuide(2, 0)).toBe(true);
    expect(showStartGuide(0, 5)).toBe(true);
    expect(showStartGuide(2, 5)).toBe(false);
  });
});

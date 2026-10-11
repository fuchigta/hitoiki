import { describe, expect, it } from "vitest";
import { mainKey, shortcutFromKey } from "./shortcut-util";

const k = (code: string, o: Partial<Record<"ctrlKey" | "altKey" | "shiftKey" | "metaKey", boolean>> = {}) => ({
  code,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...o,
});

describe("mainKey", () => {
  it("英数字・ファンクション・名前付きキー", () => {
    expect(mainKey("KeyK")).toBe("K");
    expect(mainKey("Digit5")).toBe("5");
    expect(mainKey("F9")).toBe("F9");
    expect(mainKey("F24")).toBe("F24");
    expect(mainKey("F25")).toBeNull();
    expect(mainKey("ArrowUp")).toBe("Up");
    expect(mainKey("Space")).toBe("Space");
  });
  it("修飾キーだけや未対応のキーは null", () => {
    expect(mainKey("ControlLeft")).toBeNull();
    expect(mainKey("Semicolon")).toBeNull();
  });
});

describe("shortcutFromKey", () => {
  it("修飾キーと主キーから文字列を作る", () => {
    expect(shortcutFromKey(k("Space", { ctrlKey: true, altKey: true }))).toBe("Ctrl+Alt+Space");
    expect(shortcutFromKey(k("KeyK", { ctrlKey: true, shiftKey: true, metaKey: true }))).toBe("Ctrl+Shift+Super+K");
  });
  it("修飾キーが無い、または主キーが無いときは null", () => {
    expect(shortcutFromKey(k("KeyK"))).toBeNull();
    expect(shortcutFromKey(k("ControlLeft", { ctrlKey: true }))).toBeNull();
  });
});

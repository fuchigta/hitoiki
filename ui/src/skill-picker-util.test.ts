import { describe, expect, it } from "vitest";
import type { SkillView } from "./api";
import { filterSkills } from "./skill-picker-util";

const sk = (name: string, parent_name: string | null = null) => ({ id: name, name, parent_name }) as unknown as SkillView;
const all = [sk("Rust"), sk("Rustfmt", "Rust"), sk("Trust"), sk("ＵＩデザイン"), sk("Go")];
const names = (l: SkillView[]) => l.map((s) => s.name);

describe("filterSkills", () => {
  it("空入力なら付与済みを除いて全て", () => {
    expect(names(filterSkills(all, "", ["go"]))).toEqual(["Rust", "Rustfmt", "Trust", "ＵＩデザイン"]);
  });
  it("前方一致を部分一致より先に出す", () => {
    expect(names(filterSkills(all, "rust", []))).toEqual(["Rust", "Rustfmt", "Trust"]);
  });
  it("全角半角・大文字小文字の差を吸収する", () => {
    expect(names(filterSkills(all, "ui", []))).toEqual(["ＵＩデザイン"]);
    expect(names(filterSkills(all, "ＧＯ", []))).toEqual(["Go"]);
  });
  it("付与済みは差を吸収して除く", () => {
    expect(names(filterSkills(all, "rust", ["RUST"]))).toEqual(["Rustfmt", "Trust"]);
  });
  it("一致が無ければ空", () => {
    expect(filterSkills(all, "zzz", [])).toEqual([]);
  });
});

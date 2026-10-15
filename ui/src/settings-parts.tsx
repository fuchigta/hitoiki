// 設定の部品。設定画面のタブとオンボーディングの両方から使う（設定の書き換えは configSet だけ）。
import type { ComponentChildren } from "preact";
import { api } from "./api";
import { ErrorBox } from "./components";
import { loadTimeZone, useAction, useAsync } from "./lib";
import { Help } from "./ui/parts";

export interface Cfg {
  general?: { timezone?: string; autostart?: boolean; update_check?: boolean | null };
  push?: {
    enabled?: boolean;
    idle_return_min?: number;
    urgent_task_interval_min?: number;
    important_stale_days?: number;
    kr_idle_weekday?: number;
  };
  actual?: { trust_doing?: boolean; auto_break_locked_min?: number };
}

/** 設定の読み込みと書き込み。部品は `ConfigHandle` を受け取り、保存すると全体を読み直す。 */
export function useConfig() {
  const cfg = useAsync(() => api.configAll(), []);
  const act = useAction();
  const set = async (key: string, value: string): Promise<boolean> => {
    const r = await act.run(() => api.configSet(key, value));
    if (!r) return false;
    if (key === "general.timezone") void loadTimeZone();
    cfg.reload();
    return true;
  };
  return { c: (cfg.data ?? {}) as Cfg, set, busy: act.busy, error: cfg.error ?? act.error, raw: cfg.data };
}
export type ConfigHandle = ReturnType<typeof useConfig>;

/** ログイン時の自動起動。 */
export function AutostartToggle({ cfg, label = "ログイン時に自動で起動する" }: { cfg: ConfigHandle; label?: string }) {
  return (
    <label class="check">
      <input
        type="checkbox"
        checked={cfg.c.general?.autostart === true}
        disabled={cfg.busy}
        onChange={(e) => cfg.set("general.autostart", String(e.currentTarget.checked))}
      />
      {label}
    </label>
  );
}

const UPDATE_CHECK_LABEL = "新しいバージョンを確認する（1日1回、GitHub に問い合わせます。送る情報はありません）";

/**
 * 新しいバージョンの確認。`radio` は未選択（null）を区別できる「はい／いいえ」（オンボーディング用）、
 * `check` は設定画面のチェックボックス。
 */
export function UpdateCheckToggle({
  cfg,
  variant = "check",
  help,
}: {
  cfg: ConfigHandle;
  variant?: "check" | "radio";
  help?: ComponentChildren;
}) {
  const v = cfg.c.general?.update_check ?? null;
  if (variant === "check") {
    return (
      <label class="check">
        <input
          type="checkbox"
          checked={v === true}
          disabled={cfg.busy}
          onChange={(e) => cfg.set("general.update_check", String(e.currentTarget.checked))}
        />
        {UPDATE_CHECK_LABEL}
        {help && <Help>{help}</Help>}
      </label>
    );
  }
  return (
    <fieldset class="stack">
      <legend>
        {UPDATE_CHECK_LABEL} {help && <Help>{help}</Help>}
      </legend>
      {[
        { on: true, label: "はい" },
        { on: false, label: "いいえ" },
      ].map((o) => (
        <label key={o.label} class="check">
          <input
            type="radio"
            name="update-check"
            checked={v === o.on}
            disabled={cfg.busy}
            onChange={() => cfg.set("general.update_check", String(o.on))}
          />
          {o.label}
        </label>
      ))}
    </fieldset>
  );
}

/** 始めたタスクを信頼する（actual.trust_doing）。 */
export function TrustDoingToggle({
  cfg,
  label = "始めたタスクを信頼する（進行中のタスクに仮で当てた時間も、確認待ちにしない）",
}: {
  cfg: ConfigHandle;
  label?: string;
}) {
  return (
    <label class="check">
      <input
        type="checkbox"
        checked={cfg.c.actual?.trust_doing === true}
        disabled={cfg.busy}
        onChange={(e) => cfg.set("actual.trust_doing", String(e.currentTarget.checked))}
      />
      {label}
    </label>
  );
}

/** 数値の設定 1 行。ラベル（短く）+ 入力 + 単位、説明は Help、「0 で無効」は単位の横に小さく。 */
export function NumberSetting({
  cfg,
  k,
  label,
  help,
  value,
  unit,
  zeroOff,
  max,
}: {
  cfg: ConfigHandle;
  k: string;
  label: string;
  help: ComponentChildren;
  value: number;
  unit: string;
  zeroOff?: boolean;
  max?: number;
}) {
  return (
    <div class="setting-row">
      <span class="setting-label">
        <label for={`cfg-${k}`}>{label}</label>
        <Help label={`${label}の説明`}>{help}</Help>
      </span>
      <input
        id={`cfg-${k}`}
        class="setting-input"
        type="number"
        min="0"
        max={max}
        value={value}
        disabled={cfg.busy}
        onChange={(e) => {
          const n = Math.max(0, Number(e.currentTarget.value) || 0);
          void cfg.set(k, String(max != null ? Math.min(max, n) : n));
        }}
      />
      <span class="meta">{unit}</span>
      {zeroOff && <span class="meta setting-off">0 で無効</span>}
    </div>
  );
}

export function ConfigError({ cfg }: { cfg: ConfigHandle }) {
  return <ErrorBox error={cfg.error} />;
}

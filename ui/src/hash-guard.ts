// ブラウザの戻る・進むと未保存の変更の整理（純粋関数）。

/** ハッシュが変わったときの処理。 */
export type HashStep =
  /** 新しいハッシュを受け入れる（画面を切り替えてよい）。 */
  | "accept"
  /** 何もしない（受け入れ済みと同じ。元に戻した直後のイベントもここ）。 */
  | "skip"
  /** 元のハッシュに戻し、離脱の確認を出す。 */
  | "restore";

/**
 * `hashchange` を受けたときの処理を決める。
 * 受け入れ済みのハッシュと同じなら何もしない。未保存の変更があれば元に戻して確認を挟み、なければ受け入れる。
 */
export function hashStep(accepted: string, hash: string, dirty: boolean): HashStep {
  if (hash === accepted) return "skip";
  return dirty ? "restore" : "accept";
}

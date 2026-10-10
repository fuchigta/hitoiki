import { Card, Empty } from "../components";

/** 振り返り（始業・終業・週次）。service が揃ったら実装する。今は枠だけ。 */
export function Reflect() {
  return (
    <div class="stack">
      <Card title="始業">
        <Empty>準備中です。</Empty>
      </Card>
      <Card title="終業">
        <Empty>準備中です。</Empty>
      </Card>
      <Card title="週次">
        <Empty>準備中です。</Empty>
      </Card>
    </div>
  );
}

use std::path::Path;

fn main() {
    // `ui/dist` が無いと tauri の context 生成が失敗し、`cargo test --workspace` も通らない。
    // 未ビルドのときは案内だけを表示するページを置いて、ビルド自体は通す。
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/dist");
    let index = dist.join("index.html");
    if !index.exists() {
        println!("cargo:warning=ui/dist がありません。UI を含めるには先に `cd ui && npm ci && npm run build` を実行してください。");
        std::fs::create_dir_all(&dist).expect("ui/dist を作成できません");
        std::fs::write(
            &index,
            "<!doctype html><meta charset=\"utf-8\"><title>Hitoiki</title>\
             <p>UI がビルドされていません。<code>cd ui &amp;&amp; npm ci &amp;&amp; npm run build</code> を実行してから再ビルドしてください。</p>",
        )
        .expect("プレースホルダを書けません");
    }
    println!("cargo:rerun-if-changed=../../ui/dist/index.html");
    tauri_build::build();
}

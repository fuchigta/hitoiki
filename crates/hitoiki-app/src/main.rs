// リリースビルドではコンソール窓を出さない。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    hitoiki_app::run();
}

/// 新しい ULID を文字列で返す。
pub fn new_id() -> String {
    ulid::Ulid::new().to_string()
}

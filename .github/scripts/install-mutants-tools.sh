#!/usr/bin/env bash
# ミューテーションテストで使う cargo-mutants と cargo-nextest を、公式リリースのバイナリを
# sha256 で検証してから ~/.cargo/bin に置く（cargo install のビルドを避ける）。
# 版と sha256 は mutation.yml の環境変数で渡す。
set -euo pipefail
: "${CARGO_MUTANTS_VERSION:?}" "${CARGO_MUTANTS_SHA256:?}"
: "${CARGO_NEXTEST_VERSION:?}" "${CARGO_NEXTEST_SHA256:?}"

# install <URL> <sha256> <アーカイブ内の実行ファイル名>
install_tool() {
  local url=$1 sha=$2 bin=$3 work
  work=$(mktemp -d)
  curl -fsSL -o "${work}/a.tar.gz" "$url"
  echo "${sha}  ${work}/a.tar.gz" | sha256sum -c -
  tar -xzf "${work}/a.tar.gz" --no-same-owner -C "$work"
  install -m 0755 "${work}/${bin}" "${HOME}/.cargo/bin/${bin}"
}

install_tool \
  "https://github.com/sourcefrog/cargo-mutants/releases/download/v${CARGO_MUTANTS_VERSION}/cargo-mutants-x86_64-unknown-linux-gnu.tar.gz" \
  "$CARGO_MUTANTS_SHA256" cargo-mutants
install_tool \
  "https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-${CARGO_NEXTEST_VERSION}/cargo-nextest-${CARGO_NEXTEST_VERSION}-x86_64-unknown-linux-gnu.tar.gz" \
  "$CARGO_NEXTEST_SHA256" cargo-nextest

cargo mutants --version
cargo nextest --version

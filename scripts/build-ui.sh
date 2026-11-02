#!/usr/bin/env bash
set -euo pipefail
forge_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$forge_root"
if ! rustup target list --installed | grep -qx wasm32-unknown-unknown; then
  rustup target add wasm32-unknown-unknown
fi
if [[ ! -x .tools/bin/wasm-bindgen ]]; then
  cargo install wasm-bindgen-cli --version 0.2.130 --locked --root .tools
fi
cargo build -p forge-deploy-ui --target wasm32-unknown-unknown --release
.tools/bin/wasm-bindgen target/wasm32-unknown-unknown/release/forge_deploy_ui.wasm \
  --target web --no-typescript --out-dir crates/app/assets/pkg

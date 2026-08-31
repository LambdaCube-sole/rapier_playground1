#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

export RUSTFLAGS="-C target-feature=+simd128"
pnpm build

rm -f funnel-web.zip
(cd dist && zip -r ../funnel-web.zip .)

echo
unzip -l funnel-web.zip | head
echo
echo "wrote $(pwd)/funnel-web.zip"

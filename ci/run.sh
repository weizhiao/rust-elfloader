#!/usr/bin/env sh

set -ex

: "${TARGET?The TARGET environment variable must be set.}"

if [ "${TARGET}" = "x86_64-unknown-linux-gnu"]; then
	CROSS=0
else
	CROSS=1
fi

CARGO=cargo
if [ "${CROSS}" = "1" ]; then
	export CARGO_NET_RETRY=5
	export CARGO_NET_TIMEOUT=10

	cargo install --locked cross --git https://github.com/cross-rs/cross
	CARGO=cross
fi

"${CARGO}" -vv ${OP} --target="${TARGET}" --no-default-features --features "${FEATURES}"

#!/usr/bin/env sh

set -ex

: "${TARGET?The TARGET environment variable must be set.}"

export CARGO_NET_RETRY=5
export CARGO_NET_TIMEOUT=10

cargo install --locked cross --git https://github.com/cross-rs/cross
CARGO=cross

cargo clean

if [ "${MINI_LOADER}" = "1" ]; then
	"${CARGO}" ${OP} --target="${TARGET}" ${ARGS}
else
	"${CARGO}" -vv ${OP} --target="${TARGET}" --no-default-features --features "${FEATURES}"
fi

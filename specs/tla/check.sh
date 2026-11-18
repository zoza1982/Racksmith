#!/usr/bin/env bash
# Model-checks MachineLifecycle.tla with TLC, exiting non-zero on any violated assumption,
# invariant or property, or on deadlock.
#
# Downloads a pinned, checksum-verified tla2tools.jar into .tools/ on first use. Needs bash,
# curl, Java 11+ and sha256sum (or shasum). Works from any directory.
#
# Usage: specs/tla/check.sh [extra TLC options...]
set -euo pipefail

readonly TLA_TOOLS_VERSION="v1.7.4"
readonly TLA_TOOLS_SHA256="936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"
readonly TLA_TOOLS_URL="https://github.com/tlaplus/tlaplus/releases/download/${TLA_TOOLS_VERSION}/tla2tools.jar"

spec_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly spec_dir
readonly tools_dir="${spec_dir}/.tools/${TLA_TOOLS_VERSION}"
readonly jar="${tools_dir}/tla2tools.jar"

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    else
        shasum -a 256 "$1" | cut -d ' ' -f 1
    fi
}

# Downloads to a temporary file and moves it into place only once the checksum matches, so an
# interrupted or tampered download never leaves a jar behind.
fetch_tla_tools() {
    mkdir -p "${tools_dir}"
    local partial
    partial="$(mktemp "${tools_dir}/tla2tools.jar.XXXXXX")"
    trap 'rm -f "${partial}"' EXIT

    echo "Downloading tla2tools.jar ${TLA_TOOLS_VERSION}" >&2
    curl --fail --silent --show-error --location --retry 3 --output "${partial}" "${TLA_TOOLS_URL}"

    local actual
    actual="$(sha256_of "${partial}")"
    if [[ "${actual}" != "${TLA_TOOLS_SHA256}" ]]; then
        echo "error: tla2tools.jar checksum mismatch" >&2
        echo "  expected ${TLA_TOOLS_SHA256}" >&2
        echo "  actual   ${actual}" >&2
        exit 1
    fi
    mv "${partial}" "${jar}"
    trap - EXIT
}

if [[ ! -f "${jar}" ]]; then
    fetch_tla_tools
fi

cd "${spec_dir}"
# Deadlock checking stays on: every lifecycle state has an outgoing transition (MarkBroken or
# MarkFixed), so a deadlock would mean the model is wrong. TLC exits non-zero on any failure.
exec java -XX:+UseParallelGC -cp "${jar}" tlc2.TLC \
    -workers auto \
    -cleanup \
    -metadir "${spec_dir}/states" \
    -config MachineLifecycle.cfg \
    "$@" \
    MachineLifecycle.tla

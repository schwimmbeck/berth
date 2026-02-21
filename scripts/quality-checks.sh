#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BERTH_BIN="${ROOT_DIR}/target/debug/berth"

if [[ ! -x "${BERTH_BIN}" ]]; then
  echo "Missing berth binary at ${BERTH_BIN}. Run cargo build first."
  exit 1
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT
export BERTH_HOME="${TMP_DIR}/.berth"

echo "[quality] Install check"
"${BERTH_BIN}" install github >/dev/null
"${BERTH_BIN}" config github --set token=ci-token >/dev/null

CONFIG_FILE="${BERTH_HOME}/servers/github.toml"
if [[ ! -f "${CONFIG_FILE}" ]]; then
  echo "Install check failed: missing ${CONFIG_FILE}"
  exit 1
fi

echo "[quality] Startup check"
perl -0777 -i -pe 's/command = "[^"]*"/command = "sh"/g; s/args = \[[^\]]*\]/args = ["-c", "sleep 30"]/s' "${CONFIG_FILE}"
"${BERTH_BIN}" start github >/dev/null

status_out="$("${BERTH_BIN}" status)"
if ! grep -qi "github" <<<"${status_out}" || ! grep -qi "running" <<<"${status_out}"; then
  echo "Startup check failed: expected github running in status output"
  echo "${status_out}"
  exit 1
fi
"${BERTH_BIN}" stop github >/dev/null

echo "[quality] Handshake/proxy check"
perl -0777 -i -pe 's/args = \[[^\]]*\]/args = ["-c", "echo proxy-ok"]/s' "${CONFIG_FILE}"
proxy_out="$("${BERTH_BIN}" proxy github)"
if ! grep -q "proxy-ok" <<<"${proxy_out}"; then
  echo "Handshake check failed: expected proxy output marker"
  echo "${proxy_out}"
  exit 1
fi

echo "[quality] Response time check"
start_ms="$(date +%s%3N)"
"${BERTH_BIN}" search github >/dev/null
end_ms="$(date +%s%3N)"
elapsed_ms="$((end_ms - start_ms))"
max_ms=2000
if (( elapsed_ms > max_ms )); then
  echo "Response time check failed: search took ${elapsed_ms}ms (max ${max_ms}ms)"
  exit 1
fi

echo "[quality] All quality checks passed"

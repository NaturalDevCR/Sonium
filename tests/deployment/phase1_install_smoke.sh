#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
installer="${root_dir}/install.sh"
fixture="${root_dir}/tests/fixtures/legacy-server-audio.toml"
quoted_fixture="${root_dir}/tests/fixtures/legacy-server-audio-quoted.toml"
double_header_fixture="${root_dir}/tests/fixtures/legacy-server-header-double-quoted.toml"
single_quoted_fixture="${root_dir}/tests/fixtures/legacy-server-audio-single-quoted.toml"
audio_table_fixture="${root_dir}/tests/fixtures/server-audio-quoted-keys.toml"
multiline_fixture="${root_dir}/tests/fixtures/multiline-legacy-looking-content.toml"
escaped_multiline_fixture="${root_dir}/tests/fixtures/multiline-escaped-comments.toml"
legacy_after_multiline_fixture="${root_dir}/tests/fixtures/legacy-after-multiline.toml"
invalid_toml_fixture="${root_dir}/tests/fixtures/invalid-preflight.toml"
installation_doc="${root_dir}/docs/src/getting-started/installation.md"
configuration_doc="${root_dir}/docs/src/getting-started/configuration.md"
compose_file="${root_dir}/docker-compose.yml"
docker_bootstrap="${root_dir}/deploy/docker/init-admin.sh"

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
contains() { grep -Fq -- "$2" "$1" || fail "missing '$2' in $1"; }

for legacy_fixture in "${fixture}" "${quoted_fixture}" "${double_header_fixture}" "${single_quoted_fixture}"; do
  if output="$(bash "${installer}" --preflight-config "${legacy_fixture}" 2>&1)"; then
    fail "legacy preflight unexpectedly succeeded for ${legacy_fixture##*/}"
  fi
  [[ "${output}" == *"Legacy [server] audio keys detected"* ]] \
    || fail "legacy preflight did not identify ${legacy_fixture##*/}"
  [[ "${output}" == *"Move buffer_ms, chunk_ms, and output_prefill_ms to [server.audio]"* ]] \
    || fail "legacy preflight did not provide migration instruction for ${legacy_fixture##*/}"
done

if ! output="$(bash "${installer}" --preflight-config "${audio_table_fixture}" 2>&1)"; then
  fail "[server.audio] keys unexpectedly blocked preflight: ${output}"
fi

if ! output="$(bash "${installer}" --preflight-config "${multiline_fixture}" 2>&1)"; then
  fail "multiline string content unexpectedly blocked preflight: ${output}"
fi

if ! output="$(bash "${installer}" --preflight-config "${escaped_multiline_fixture}" 2>&1)"; then
  fail "escaped multiline string content unexpectedly blocked preflight: ${output}"
fi

if output="$(bash "${installer}" --preflight-config "${legacy_after_multiline_fixture}" 2>&1)"; then
  fail "legacy keys after a multiline string unexpectedly passed preflight"
fi
[[ "${output}" == *"Legacy [server] audio keys detected"* ]] \
  || fail "legacy keys after a multiline string were not detected"

if output="$(bash "${installer}" --preflight-config "${invalid_toml_fixture}" 2>&1)"; then
  fail "malformed TOML unexpectedly passed preflight"
fi
[[ "${output}" == *"Could not parse existing configuration"* ]] \
  || fail "malformed TOML did not produce an actionable preflight error"

if output="$(PATH=/nonexistent /bin/bash "${installer}" --preflight-config "${audio_table_fixture}" 2>&1)"; then
  fail "preflight unexpectedly ran without python3/tomllib"
fi
[[ "${output}" == *"Python 3.11+ with tomllib is required"* ]] \
  || fail "missing python3/tomllib did not produce an actionable preflight error"

# On a non-root test host this invocation stops at the platform/root guard,
# before any uninstall action. It must not be rejected by legacy preflight.
if [[ "${EUID}" -ne 0 ]]; then
  if output="$(bash "${installer}" --uninstall --preflight-config "${fixture}" 2>&1)"; then
    fail "uninstall test unexpectedly continued past the platform/root guard"
  fi
  [[ "${output}" != *"Legacy [server] audio keys detected"* ]] \
    || fail "--uninstall must bypass legacy configuration preflight"
fi

if output="$(sh "${docker_bootstrap}" 2>&1)"; then
  fail "Docker bootstrap unexpectedly accepted an unset password"
fi
[[ "${output}" == *"SONIUM_INIT_ADMIN_PASSWORD must be set"* ]] \
  || fail "Docker bootstrap did not reject an unset password"

if grep -RE -- '--init-admin[[:space:]]+[^>;[:space:]]' \
  "${installer}" "${docker_bootstrap}"; then
  fail "a first-party bootstrap still passes a plaintext password in argv"
fi
if awk '
  /^```(bash|sh|shell|powershell)$/ { in_block = 1; next }
  in_block && /^```$/ { in_block = 0; next }
  in_block { print }
' "${root_dir}/README.md" "${installation_doc}" \
  "${root_dir}/docs/src/getting-started/quick-start.md" \
  | grep -E -- '--init-admin[[:space:]]+[^>;[:space:]]'; then
  fail "a documented bootstrap still passes a plaintext password in argv"
fi

contains "${compose_file}" "init-admin:"
contains "${compose_file}" "profiles: [\"bootstrap\"]"
contains "${compose_file}" "SONIUM_INIT_ADMIN_PASSWORD"
contains "${installation_doc}" "if docker compose --profile bootstrap run --rm init-admin; then"
contains "${installation_doc}" "Read-Host -AsSecureString"
if grep -Fq -- "choose-a-strong-password" "${installation_doc}"; then
  fail "installation documentation still places an admin password in a command"
fi

bootstrap_line="$(grep -n 'if docker compose --profile bootstrap run --rm init-admin; then' "${installation_doc}" | head -n1 | cut -d: -f1)"
up_line="$(grep -n 'docker compose up -d' "${installation_doc}" | head -n1 | cut -d: -f1)"
[[ -n "${bootstrap_line}" && -n "${up_line}" && "${bootstrap_line}" -lt "${up_line}" ]] \
  || fail "Compose documentation must bootstrap admin before docker compose up"

contains "${installer}" "Legacy [server] audio keys detected"
contains "${installer}" "Could not initialize the initial admin account"
contains "${installer}" "unset GEN_PASS"

timezone_line="$(grep -n '^timezone = ' "${configuration_doc}" | head -n1 | cut -d: -f1)"
streams_line="$(grep -n '^\[\[streams\]\]' "${configuration_doc}" | head -n1 | cut -d: -f1)"
[[ -n "${timezone_line}" && -n "${streams_line}" && "${timezone_line}" -lt "${streams_line}" ]] \
  || fail "timezone must be declared before [[streams]]"
contains "${configuration_doc}" "rtp_udp/rist use 0 = stream_port + 2"
contains "${configuration_doc}" "ordinary file/FIFO sources after a recoverable"

printf 'phase1 deployment smoke: PASS\n'

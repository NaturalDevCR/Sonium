#!/bin/sh
set -eu

if [ -z "${SONIUM_INIT_ADMIN_PASSWORD:-}" ]; then
  printf '%s\n' 'SONIUM_INIT_ADMIN_PASSWORD must be set for the bootstrap profile.' >&2
  exit 64
fi

password="${SONIUM_INIT_ADMIN_PASSWORD}"
unset SONIUM_INIT_ADMIN_PASSWORD
if printf '%s' "${password}" | sonium-server --config /etc/sonium/sonium.toml --init-admin; then
  status=0
else
  status=$?
fi
unset password
exit "${status}"

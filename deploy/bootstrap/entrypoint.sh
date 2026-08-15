#!/bin/sh
set -eu

umask 077

secrets_dir=${FOYER_BOOTSTRAP_SECRETS_DIR:-/secrets}
signing_key=${secrets_dir}/token-signing.pem
radicale_users=${secrets_dir}/radicale-users
signing_key_tmp=""
radicale_users_tmp=""

trap 'rm -f "${signing_key_tmp}" "${radicale_users_tmp}"' EXIT INT HUP TERM

if [ -z "${FOYER_DAV_PASSWORD:-}" ]; then
  echo "FOYER_DAV_PASSWORD is required" >&2
  exit 1
fi

mkdir -p "${secrets_dir}"
chmod 0755 "${secrets_dir}"

if [ ! -s "${signing_key}" ]; then
  signing_key_tmp=$(mktemp "${secrets_dir}/.token-signing.XXXXXX")
  openssl ecparam -name prime256v1 -genkey -noout -out "${signing_key_tmp}"
  chown 10001:10001 "${signing_key_tmp}"
  chmod 0400 "${signing_key_tmp}"
  mv -f "${signing_key_tmp}" "${signing_key}"
  signing_key_tmp=""
  echo "created the persistent Foyer ES256 signing key"
else
  echo "kept the existing persistent Foyer ES256 signing key"
fi

radicale_users_tmp=$(mktemp "${secrets_dir}/.radicale-users.XXXXXX")
printf '%s\n' "${FOYER_DAV_PASSWORD}" | htpasswd -niBC 12 foyer >"${radicale_users_tmp}"
chmod 0444 "${radicale_users_tmp}"
mv -f "${radicale_users_tmp}" "${radicale_users}"
radicale_users_tmp=""
echo "wrote the Radicale service-account password file"

trap - EXIT INT HUP TERM

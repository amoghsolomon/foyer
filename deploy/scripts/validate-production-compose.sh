#!/bin/sh
set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
deploy_dir=$(CDPATH='' cd -- "${script_dir}/.." && pwd)
env_file=$(mktemp)
trap 'rm -f "${env_file}"' EXIT INT HUP TERM
umask 077

"${script_dir}/generate-production-env.sh" >"${env_file}"
docker compose \
  --env-file "${env_file}" \
  -f "${deploy_dir}/compose.production.yaml" \
  --profile backup \
  config --quiet

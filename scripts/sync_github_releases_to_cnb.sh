#!/usr/bin/env bash
set -euo pipefail

: "${CNB_REPO_SLUG:?missing CNB_REPO_SLUG}"
: "${CNB_API_TOKEN:?missing CNB_API_TOKEN}"
: "${CNB_USERNAME:?missing CNB_USERNAME}"
: "${GH_REPO:?missing GH_REPO}"
: "${GH_TOKEN:?missing GH_TOKEN}"
: "${RELEASE_TAG:?missing RELEASE_TAG}"
: "${SYNC_CHANNEL:?missing SYNC_CHANNEL}"

UPDATE_SIGNING_KEY_ID="${UPDATE_SIGNING_KEY_ID:-gproxy-release-v1}"
CNB_PRERELEASE="${CNB_PRERELEASE:-false}"
CNB_MAKE_LATEST="${CNB_MAKE_LATEST:-legacy}"

CNB_API_BASE="https://api.cnb.cool/${CNB_REPO_SLUG}/-/releases"
GH_API_BASE="https://api.github.com/repos/${GH_REPO}/releases"

work_dir="$(mktemp -d)"
trap 'rm -rf "${work_dir}"' EXIT

gh_release_json="${work_dir}/gh-release.json"
manifest_assets_jsonl="${work_dir}/manifest-assets.jsonl"
: > "${manifest_assets_jsonl}"

curl -fsSL \
  -H "Accept: application/vnd.github+json" \
  -H "Authorization: Bearer ${GH_TOKEN}" \
  -H "X-GitHub-Api-Version: 2022-11-28" \
  "${GH_API_BASE}/tags/${RELEASE_TAG}" \
  -o "${gh_release_json}"

gh_release_name="$(jq -r '.name // .tag_name // ""' "${gh_release_json}")"
gh_release_body="$(jq -r '.body // ""' "${gh_release_json}")"
gh_release_target="$(jq -r '.target_commitish // ""' "${gh_release_json}")"
asset_count="$(jq '.assets | length' "${gh_release_json}")"
if [ "${asset_count}" -eq 0 ]; then
  echo "no assets found on GitHub release tag ${RELEASE_TAG}" >&2
  exit 1
fi

cnb_release_json="${work_dir}/cnb-release.json"
get_status="$(curl -sS -o "${cnb_release_json}" -w '%{http_code}' \
  -H "Accept: application/vnd.cnb.api+json" \
  -H "Authorization: Bearer ${CNB_API_TOKEN}" \
  "${CNB_API_BASE}/tags/${RELEASE_TAG}")"

if [ "${get_status}" = "200" ]; then
  cnb_release_id="$(jq -r '.id // ""' "${cnb_release_json}")"
elif [ "${get_status}" = "404" ]; then
  create_payload="$(jq -n \
    --arg tag "${RELEASE_TAG}" \
    --arg name "${gh_release_name}" \
    --arg body "${gh_release_body}" \
    --arg target "${gh_release_target}" \
    --arg make_latest "${CNB_MAKE_LATEST}" \
    --argjson prerelease "${CNB_PRERELEASE}" \
    '{tag_name:$tag,name:$name,body:$body,target_commitish:$target,prerelease:$prerelease,draft:false,make_latest:$make_latest}')"
  create_status="$(curl -sS -o "${cnb_release_json}" -w '%{http_code}' \
    -X POST \
    -H "Accept: application/vnd.cnb.api+json" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer ${CNB_API_TOKEN}" \
    -d "${create_payload}" \
    "${CNB_API_BASE}")"
  if [ "${create_status}" != "201" ]; then
    echo "failed to create CNB release ${RELEASE_TAG}, status=${create_status}" >&2
    cat "${cnb_release_json}" >&2
    exit 1
  fi
  cnb_release_id="$(jq -r '.id // ""' "${cnb_release_json}")"
else
  echo "failed to get CNB release ${RELEASE_TAG}, status=${get_status}" >&2
  cat "${cnb_release_json}" >&2
  exit 1
fi

if [ -z "${cnb_release_id}" ]; then
  echo "empty CNB release id for tag ${RELEASE_TAG}" >&2
  exit 1
fi

jq -c '.assets[] | {id: .id, name: .name, size: (.size // 0)}' "${gh_release_json}" | \
while IFS= read -r asset; do
  asset_id="$(jq -r '.id' <<< "${asset}")"
  asset_name="$(jq -r '.name' <<< "${asset}")"
  asset_size="$(jq -r '.size' <<< "${asset}")"
  asset_file="${work_dir}/${asset_name}"
  asset_api_url="https://api.github.com/repos/${GH_REPO}/releases/assets/${asset_id}"

  echo "sync asset: ${asset_name}"

  curl -fsSL \
    -L \
    -H "Accept: application/octet-stream" \
    -H "Authorization: Bearer ${GH_TOKEN}" \
    -H "X-GitHub-Api-Version: 2022-11-28" \
    "${asset_api_url}" \
    -o "${asset_file}"

  if [ ! -s "${asset_file}" ]; then
    echo "downloaded empty asset: ${asset_name}" >&2
    exit 1
  fi

  if [ "${asset_size}" -le 0 ]; then
    asset_size="$(wc -c < "${asset_file}")"
  fi

  upload_resp="$(mktemp)"
  upload_payload="$(jq -n \
    --arg asset_name "${asset_name}" \
    --argjson size "${asset_size}" \
    '{asset_name:$asset_name,size:$size,overwrite:true}')"
  upload_status="$(curl -sS -o "${upload_resp}" -w '%{http_code}' \
    -X POST \
    -H "Accept: application/vnd.cnb.api+json" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer ${CNB_API_TOKEN}" \
    -d "${upload_payload}" \
    "${CNB_API_BASE}/${cnb_release_id}/asset-upload-url")"
  if [ "${upload_status}" != "201" ]; then
    echo "failed to request upload url for ${asset_name}, status=${upload_status}" >&2
    cat "${upload_resp}" >&2
    exit 1
  fi

  upload_url="$(jq -r '.upload_url // ""' "${upload_resp}")"
  verify_url="$(jq -r '.verify_url // ""' "${upload_resp}")"
  rm -f "${upload_resp}"

  if [ -z "${upload_url}" ] || [ -z "${verify_url}" ]; then
    echo "missing upload_url/verify_url for ${asset_name}" >&2
    exit 1
  fi

  curl -fsSL \
    -X PUT \
    -H "Content-Type: application/octet-stream" \
    --upload-file "${asset_file}" \
    "${upload_url}" >/dev/null

  verify_with_ttl="${verify_url}"
  if [ -n "${CNB_RELEASE_ASSET_TTL:-}" ]; then
    if [[ "${verify_with_ttl}" == *\?* ]]; then
      verify_with_ttl="${verify_with_ttl}&ttl=${CNB_RELEASE_ASSET_TTL}"
    else
      verify_with_ttl="${verify_with_ttl}?ttl=${CNB_RELEASE_ASSET_TTL}"
    fi
  fi

  curl -fsSL \
    -X POST \
    -H "Accept: application/vnd.cnb.api+json" \
    -H "Authorization: Bearer ${CNB_API_TOKEN}" \
    "${verify_with_ttl}" >/dev/null

  if [[ "${asset_name}" == *.zip ]]; then
    encoded_name="$(jq -rn --arg v "${asset_name}" '$v|@uri')"
    download_url="https://cnb.cool/${CNB_REPO_SLUG}/-/releases/download/${RELEASE_TAG}/${encoded_name}"
    jq -nc \
      --arg name "${asset_name}" \
      --arg url "${download_url}" \
      --arg sha256_url "${download_url}.sha256" \
      --arg sha256_sig_url "${download_url}.sha256.sig" \
      --arg key_id "${UPDATE_SIGNING_KEY_ID}" \
      '{name:$name,url:$url,sha256_url:$sha256_url,sha256_sig_url:$sha256_sig_url,key_id:$key_id}' \
      >> "${manifest_assets_jsonl}"
  fi
done

if [ ! -s "${manifest_assets_jsonl}" ]; then
  echo "no zip assets found for manifest on tag ${RELEASE_TAG}" >&2
  exit 1
fi

manifest_assets_json="$(jq -s '.' "${manifest_assets_jsonl}")"
channel_manifest="${work_dir}/manifest-${SYNC_CHANNEL}.json"
generated_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
jq -n \
  --arg tag "${RELEASE_TAG}" \
  --arg channel "${SYNC_CHANNEL}" \
  --arg key_id "${UPDATE_SIGNING_KEY_ID}" \
  --arg generated_at "${generated_at}" \
  --argjson assets "${manifest_assets_json}" \
  '{tag:$tag,channel:$channel,key_id:$key_id,generated_at:$generated_at,assets:$assets}' \
  > "${channel_manifest}"

cnb_repo_dir="${work_dir}/cnb-repo"
git clone --depth 1 "https://${CNB_USERNAME}:${CNB_API_TOKEN}@cnb.cool/${CNB_REPO_SLUG}.git" "${cnb_repo_dir}"
default_branch="$(git -C "${cnb_repo_dir}" symbolic-ref --short refs/remotes/origin/HEAD 2>/dev/null | sed 's@^origin/@@')"
if [ -z "${default_branch}" ]; then
  default_branch="main"
fi

mkdir -p "${cnb_repo_dir}/${SYNC_CHANNEL}"
cp "${channel_manifest}" "${cnb_repo_dir}/${SYNC_CHANNEL}/manifest.json"

release_assets='[]'
if [ -f "${cnb_repo_dir}/releases/manifest.json" ]; then
  release_assets="$(jq -c '[ (.assets // [])[] | . + {path:("releases/" + .name)} ]' "${cnb_repo_dir}/releases/manifest.json")"
fi
staging_assets='[]'
if [ -f "${cnb_repo_dir}/staging/manifest.json" ]; then
  staging_assets="$(jq -c '[ (.assets // [])[] | . + {path:("staging/" + .name)} ]' "${cnb_repo_dir}/staging/manifest.json")"
fi
root_assets="$(jq -cn --argjson releases "${release_assets}" --argjson staging "${staging_assets}" '($releases + $staging) | sort_by(.path)')"
jq -n \
  --arg generated_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --argjson assets "${root_assets}" \
  '{generated_at:$generated_at,assets:$assets}' \
  > "${cnb_repo_dir}/manifest.json"

git -C "${cnb_repo_dir}" config user.name "github-actions[bot]"
git -C "${cnb_repo_dir}" config user.email "41898282+github-actions[bot]@users.noreply.github.com"
git -C "${cnb_repo_dir}" add manifest.json "${SYNC_CHANNEL}/manifest.json"
if git -C "${cnb_repo_dir}" diff --cached --quiet; then
  echo "no manifest changes to commit"
  exit 0
fi
git -C "${cnb_repo_dir}" commit -m "chore(cnb): update ${SYNC_CHANNEL} manifest (${RELEASE_TAG})"

pushed=0
for attempt in 1 2 3; do
  if git -C "${cnb_repo_dir}" push origin "HEAD:${default_branch}"; then
    pushed=1
    break
  fi
  if [ "${attempt}" -eq 3 ]; then
    break
  fi
  git -C "${cnb_repo_dir}" fetch origin "${default_branch}"
  git -C "${cnb_repo_dir}" rebase "origin/${default_branch}"
done

if [ "${pushed}" -ne 1 ]; then
  echo "failed to push manifest updates to CNB after retries" >&2
  exit 1
fi

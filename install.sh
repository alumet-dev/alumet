#!/bin/sh
set -e

usage() {
  this=$1
  cat <<EOF
$this: download alumet-agent from ${OWNER}/${REPO}

Usage: $this [-d] [-t <tag>]
  -d turns on debug logging
  -t <tag> is a tag from
   https://github.com/${OWNER}/${REPO}/releases
   If tag is missing, then the latest will be used.

EOF
  exit 2
}
parse_args() {
  while getopts "dh?xt:" arg; do
    case "$arg" in
      d) log_set_priority 10 ;;
      h | \?) usage "$0" ;;
      x) set -x ;;
      t) TAG=$OPTARG;;
    esac
  done
  
}
execute() {
  tmpdir=$(mktemp -d)
  http_download "${tmpdir}/${PACKAGE_ID}" "${PACKAGE_URL}"
  verify_hash "${tmpdir}/${PACKAGE_ID}"
  case $DISTRIB in
    ubuntu*|debian*) sudo apt-get install -yq --allow-downgrades "${tmpdir}/${PACKAGE_ID}" > "/dev/null";;
    *)  yum install -yq "${tmpdir}/${PACKAGE_ID}" > "/dev/null";;
  esac
  log_info "Installed Alumet successfully"
  rm -rf "${tmpdir}"
}
tag_to_version() {
  if [ -z "${TAG}" ]; then
    log_info "Checking GitHub for latest tag"
  else
    log_info "Checking GitHub for tag '${TAG}'"
  fi

  REALTAG=$(github_release "$OWNER/$REPO" "${TAG}") && true
  if test -z "$REALTAG"; then
    log_crit "Unable to find '${TAG}' - use 'latest' or see https://github.com/${OWNER}/${REPO}/releases for details"
    exit 1
  fi
  # if version starts with 'v', remove it
  TAG="$REALTAG"
  VERSION=${TAG#v}
}
verify_hash() {
  package_hashed=$(hash_sha256 "$1")
  log_debug "Comparing digests expected ${CHECKSUM} vs obtained ${package_hashed}"
  if [ "$package_hashed" != "$CHECKSUM" ]; then
    log_err "hash_sha256_verify checksum for '$1' did not verify ${package_hashed} vs $CHECKSUM"
    return 1
  fi
}
log_prefix() {
	echo "$PREFIX"
}

cat /dev/null <<EOF
------------------------------------------------------------------------
https://github.com/client9/shlib - portable posix shell functions
Public domain - http://unlicense.org
https://github.com/client9/shlib/blob/HEAD/LICENSE.md
but credit (and pull requests) appreciated.
------------------------------------------------------------------------
EOF
is_command() {
  command -v "$1" >/dev/null
}
echoerr() {
  echo "$@" 1>&2
}
_logp=6
log_set_priority() {
  _logp="$1"
}
log_priority() {
  if test -z "$1"; then
    echo "$_logp"
    return
  fi
  [ "$1" -le "$_logp" ]
}
log_tag() {
  case $1 in
    0) echo "emerg" ;;
    1) echo "alert" ;;
    2) echo "crit" ;;
    3) echo "err" ;;
    4) echo "warning" ;;
    5) echo "notice" ;;
    6) echo "info" ;;
    7) echo "debug" ;;
    *) echo "$1" ;;
  esac
}
log_debug() {
  log_priority 7 || return 0
  echoerr "$(log_prefix)" "$(log_tag 7)" "$@"
}
log_info() {
  log_priority 6 || return 0
  echoerr "$(log_prefix)" "$(log_tag 6)" "$@"
}
log_err() {
  log_priority 3 || return 0
  echoerr "$(log_prefix)" "$(log_tag 3)" "$@"
}
log_crit() {
  log_priority 2 || return 0
  echoerr "$(log_prefix)" "$(log_tag 2)" "$@"
}

hash_sha256() {
  TARGET=${1:-/dev/stdin}
  if is_command gsha256sum; then
    hash=$(gsha256sum "$TARGET") || return 1
    echo "$hash" | cut -d ' ' -f 1
  elif is_command sha256sum; then
    hash=$(sha256sum "$TARGET") || return 1
    echo "$hash" | cut -d ' ' -f 1
  elif is_command shasum; then
    hash=$(shasum -a 256 "$TARGET" 2>/dev/null) || return 1
    echo "$hash" | cut -d ' ' -f 1
  elif is_command openssl; then
    hash=$(openssl -dst openssl dgst -sha256 "$TARGET") || return 1
    echo "$hash" | cut -d ' ' -f a
  else
    log_crit "hash_sha256 unable to find command to compute sha-256 hash"
    return 1
  fi
}
http_download_curl() {
  local_file=$1
  source_url=$2
  header=$3
  # workaround https://github.com/curl/curl/issues/13845
  curl_version=$(curl --version | head -n 1 | awk '{ print $2 }')
  if [ "$curl_version" = "8.8.0" ]; then
    log_debug "http_download_curl curl $curl_version detected"
    if [ -z "$header" ]; then
      curl -sL -o "$local_file" "$source_url"
    else
      curl -sL -H "$header" -o "$local_file" "$source_url"

      nf=$(cat "$local_file" | jq -r '.error // ""')
      if  [ ! -z "$nf" ]; then
        log_err "http_download_curl received an error: $nf"
        return 1
      fi
    fi

    return 0
  fi

  if [ -z "$header" ]; then
      code=$(curl -w '%{http_code}' -sL -o "$local_file" "$source_url")
  else
    code=$(curl -w '%{http_code}' -sL -H "$header" -o "$local_file" "$source_url")
  fi

  if [ "$code" != "200" ]; then
    log_err "http_download_curl received HTTP status $code"
    return 1
  fi
  return 0
}
http_download_wget() {
  local_file=$1
  source_url=$2
  header=$3
  if [ -z "$header" ]; then
    wget_output=$(wget --server-response --quiet -O "$local_file" "$source_url" 2>&1)
  else
    wget_output=$(wget --server-response --quiet --header "$header" -O "$local_file" "$source_url" 2>&1)
  fi
  wget_exit=$?
  if [ $wget_exit -ne 0 ]; then
    log_err "http_download_wget failed: wget exited with status $wget_exit"
    return 1
  fi
  code=$(echo "$wget_output" | awk '/^  HTTP/{print $2}' | tail -n1)
  if [ "$code" != "200" ]; then
    log_err "http_download_wget received HTTP status $code"
    return 1
  fi
  return 0
}
http_download() {
  log_debug "http_download $2"
  if is_command curl; then
    http_download_curl "$@"
    return
  elif is_command wget; then
    http_download_wget "$@"
    return
  fi
  log_crit "http_download unable to find wget or curl"
  return 1
}
http_copy() {
  tmp=$(mktemp)
  http_download "${tmp}" "$1" "$2" || return 1
  body=$(cat "$tmp")
  rm -f "${tmp}"
  echo "$body"
}
github_release() {
  owner_repo=$1
  version=$2

  test -z "$version" && version="latest"
  giturl="https://github.com/${owner_repo}/releases/${version}"
  json=$(http_copy "$giturl" "Accept:application/json")
  test -z "$json" && return 1
  version=$(echo "$json" | tr -s '\n' ' ' | sed 's/.*"tag_name":"//' | sed 's/".*//')
  test -z "$version" && return 1
  echo "$version"
}
cat /dev/null <<EOF
------------------------------------------------------------------------
End of functions from https://github.com/client9/shlib
------------------------------------------------------------------------
EOF

check_os() {
    os=$(uname -s | tr '[:upper:]' '[:lower:]')
    case $os in
        linux) os="linux";;
        *) 
            log_err "OS not compatible (found $os), Alumet only exists for Linux"
            return 1;;
    esac
}
uname_arch() {
    arch=$(uname -m)
    case $DISTRIB in
      ubuntu*|debian*)  
        case $arch in
          x86_64) arch="amd64" ;;
          aarch64) arch="arm64" ;;
          *)
            log_err "Architecture not compatible (found $arch), Alumet only exists for x86_64 and ARM64"
            return 1;;
        esac;;
      *)
        case $arch in
          x86_64);;
          aarch64);;
          *)
            log_err "Architecture not compatible (found $arch), Alumet only exists for x86_64 and ARM64"
            return 1;;
        esac;;
    esac
    echo "${arch}"
}
get_distrib() {
    distrib=$(env -i bash -c '. /etc/os-release; echo $ID' | tr '[:upper:]' '[:lower:]')
    version=$(env -i bash -c '. /etc/os-release; echo $VERSION_ID' | tr '[:upper:]' '[:lower:]')
    case $distrib in 
        rhel) distrib="ubi${version}";;
        fedora) distrib="fc${version}";;
        ubuntu) distrib="${distrib}_${version}";;
        debian) distrib="${distrib}_${version}";;
        *)
            log_err "Your distribution doesn't have a prebuilt package. Download the alumet-agent and compile it yourself at:
                https://github.com/${OWNER}/${REPO}"
            return 1;;
    esac
    echo "${distrib}"
}
build_name(){
  case $DISTRIB in
    ubi*|fc*) name="${AGENT}_${DISTRIB}_${ARCH}.rpm";;
    ubuntu*|debian*) name="${AGENT}_${DISTRIB}_${ARCH}.deb";;
  esac
  echo "${name}"
}

find_pkg_checksum() {
  if is_command curl; then
    find_pkg_checksum_curl
    return
  elif is_command wget; then
    find_pkg_checksum_wget
    return
  fi
  log_crit "find_pkg_checksum unable to find wget or curl"
  return 1
}
find_pkg_checksum_curl() {
  res=$(curl -s "https://api.github.com/repos/${OWNER}/${REPO}/releases/tags/${TAG}" \
| awk -F'"' -v name="$AGENT" -v os="${DISTRIB}" -v arch="$ARCH" '
BEGIN {
  version_pattern = "[0-9.-]+"
  pattern_dot = name "\\-" version_pattern "\\." os "\\." arch "\\.rpm$"
  pattern_underscore = name "_" version_pattern "_" arch "_" os"\\.deb$"
}
 
# Field detection
/"name"/ { asset_name = $4 }
/"browser_download_url"/ { url = $4 }
/"digest"/ { digest = $4 }
 
# When all three fields are fullfilled
(asset_name && url && digest) {
    # Check for pattern
    if (asset_name ~ pattern_dot || asset_name ~ pattern_underscore) {
      print url, digest
  }
  asset_name=""; url=""; digest="";
}')
  PACKAGE_URL=$(echo "$res" | cut -d ' ' -f 1) 
  CHECKSUM=$(echo "$res" | cut -d ' ' -f 2  | awk '{gsub(/^.{7}/,"");}1' )

  log_debug "Checksum found: $CHECKSUM"
  log_debug "File found: $PACKAGE_URL"
}

find_pkg_checksum_wget() {
  wget --quiet "https://api.github.com/repos/${OWNER}/${REPO}/releases/tags/${TAG}" \
| awk -F'"' -v name="$AGENT" -v os="${DISTRIB}" -v arch="$ARCH" '
BEGIN {
 
  version_pattern = "[0-9.-]+"
  pattern_dot = name "\\-" version_pattern "\\." os "\\." arch "\\.rpm$"
  pattern_underscore = name "_" version_pattern "_" arch "_" os"\\.deb$"
}
 
# Field detection
/"name"/ { asset_name = $4 }
/"browser_download_url"/ { url = $4 }
/"digest"/ { digest = $4 }
 
# When all three fields are fullfilled
(asset_name && url && digest) {
    # Check for pattern
    if (asset_name ~ pattern_dot || asset_name ~ pattern_underscore) {
      print url, digest
  }
  asset_name=""; url=""; digest="";
}'
  PACKAGE_URL=$(echo "$res" | cut -d ' ' -f 1) 
  CHECKSUM=$(echo "$res" | cut -d ' ' -f 2  | awk '{gsub(/^.{7}/,"");}1' )

  log_debug "Checksum found: $CHECKSUM"
  log_debug "File found: $PACKAGE_URL"
}

OWNER="alumet-dev"
REPO="alumet"
PREFIX="$OWNER/$REPO"

AGENT="alumet-agent"

check_os

DISTRIB=$(get_distrib)
ARCH=$(uname_arch)

PACKAGE_ID=$(build_name)

parse_args "$@"

tag_to_version

log_info "Alumet version found: ${VERSION} for architecture ${DISTRIB}/${ARCH}"

find_pkg_checksum

execute
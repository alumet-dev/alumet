#!/bin/sh
set -e

usage() {
  this=$1
  cat <<EOF
$this: remove the installed alumet-agent from ${OWNER}/${REPO}

Usage: $this [-v] [-l]
  -v enables verbose logging
  -l remove the local alumet-agent installation from ~/.local/bin.
EOF
  exit 2
}
parse_args() {
  while getopts "vh?xl" arg; do
    case "$arg" in
      v) log_set_priority 10 ;;
      h | \?) usage "$0" ;;
      x) set -x ;;
      l) LOCAL="local";;
    esac
  done
}
remove_local() {
  if [ ! -d "${HOME}/.local/bin" ]; then
    log_err "directory ${HOME}/.local/bin not found"
    log_err "You may have not installed alumet with the install script locally"
    log_err "Or you may have already uninstalled it"
    return 1
  fi
  if [ ! -f "${HOME}/.local/bin/alumet-agent-local" ]; then
    log_err "file ${HOME}/.local/bin/alumet-agent-local not found"
    log_err "You may have not installed alumet with the install script locally"
    log_err "Or you may have already uninstalled it"
    return 1
  fi
  if rm "${HOME}/.local/bin/alumet-agent-local" ; then
    return 0
  fi
  log_err "Failed to delete alumet-agent-local"
  return 1
}
execute() {
  if test "$LOCAL"; then
    log_info "trying to remove locally from ~/.local/bin"
    if remove_local ; then
        log_info "Removed local Alumet successfully"
        return 0
    fi
    log_err "Failed to remove local Alumet"
    return 1
  fi
  case $DISTRIB in
    ubuntu|debian)
      sudo apt-get remove -y alumet-agent || return 1
      log_info "Removed Alumet package successfully"
      log_info "You may have residual config file !"
      log_info "For Ubuntu/Debian:"
      log_info "To list packages that have been removed but still have configuration files left behind ([residual-config]):"
      log_info "sudo apt list '~c'"
      log_info "To remove the configuration files for alumet-agent:"
      log_info "sudo apt purge alumet-agent";;
    fc|ubi)
      sudo yum remove -y alumet-agent || return 1
      log_info "Removed Alumet package successfully";;
  esac
  return 0
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
cat /dev/null <<EOF
------------------------------------------------------------------------
End of functions from https://github.com/client9/shlib
------------------------------------------------------------------------
EOF

check_os() {
    os=$(uname -s | tr '[:upper:]' '[:lower:]')
    case $os in
        linux)
          os="linux"
          return 0;;
        *)
            log_err "OS not compatible (found $os), Alumet only supports Linux"
            return 1;;
    esac
}

uname_distrib() {
    distrib=$(bash -c '. /etc/os-release; echo $ID' | tr '[:upper:]' '[:lower:]')
    case $distrib in
        rhel) distrib="ubi";;
        fedora) distrib="fc";;
        ubuntu|debian);;
        *)
            log_err "Unknown distrib"
            return 1;;
    esac
    echo "${distrib}"
}

OWNER="alumet-dev"
REPO="alumet"
PREFIX="$OWNER/$REPO"

parse_args "$@"
check_os || exit 1
DISTRIB=$(uname_distrib) || exit 1
execute || exit 1

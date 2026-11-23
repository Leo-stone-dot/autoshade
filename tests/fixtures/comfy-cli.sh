#!/bin/sh
set -eu
PATH=/usr/bin:/bin
export PATH
root=$AUTOSHADE_TEST_COMFY_ROOT

case "${1:-}" in --workspace=*) shift ;; esac
case "${1:-}" in --json) shift ;; esac
operation=${1:?missing operation}
shift
case "$operation" in
  env)
    printf '%s\n' '{"ok":true,"data":{"server":{"running":true}}}'
    ;;
  upload)
    captured=$(mktemp "$root/upload.XXXXXX")
    cp "$1" "$captured" 2> "$captured.error"
    printf '{"ok":true,"data":{"uploads":[{"name":"%s"}]}}\n' "${captured##*/}"
    ;;
  run)
    workflow=""
    while [ "$#" -gt 0 ]; do
      case "$1" in
        --workflow) workflow=$2; shift 2 ;;
        --wait) shift ;;
        *) printf '%s\n' "unsupported run argument: $1" >&2; exit 2 ;;
      esac
    done
    test -n "$workflow"
    if [ -f "$root/hold" ]; then
      ticket=$(mktemp -d "$root/job.XXXXXX")
      mkfifo "$ticket/release"
      printf '%s\n' "${ticket##*/}" > "$root/ready"
      IFS= read -r release < "$ticket/release"
    fi
    cat "$workflow"
    if [ -f "$root/fail-run" ]; then
      printf '%s\n' 'synthetic generation stopped' >&2
      exit 7
    fi
    ;;
  download)
    out=""
    while [ "$#" -gt 0 ]; do
      case "$1" in
        --out-dir) out=$2; shift 2 ;;
        *) printf '%s\n' "unsupported download argument: $1" >&2; exit 2 ;;
      esac
    done
    test -n "$out"
    captured=$(mktemp "$root/workflow.XXXXXX")
    cat > "$captured"
    cp "$root/answer.png" "$out/generated.png"
    ;;
  *)
    printf '%s\n' "unsupported fixture operation: $operation" >&2
    exit 2
    ;;
esac

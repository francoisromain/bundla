#!/usr/bin/env bash
set -e
cd "$(dirname "$0")"

cargo build
watchexec -w src cargo build &
watcher_pid=$!
trap 'kill "$watcher_pid" 2>/dev/null || true' EXIT INT TERM

webadev --dir ./dist

#!/bin/bash

FSS_PORT=8080
FSS_HOST="localhost:$FSS_PORT"
FSS_URL="http://$FSS_HOST/v1/signals"

oneTimeSetUp() {
  cargo build
  nohup "${CARGO_TARGET_DIR:-./target}/debug/ficai-signals-server" >test.log 2>&1 &
  echo $! >test.pid
  echo "server process pid: $(cat test.pid)"
  for i in {1..5} ; do
    sleep 0.1s
    pgrep -F test.pid >/dev/null && break
    [[ "$i" -eq 5 ]] && echo "tired of waiting for server to start" && exit 1
  done
}

oneTimeTearDown() {
  # shunit2 2.1.8 runs teardown once more at the end of execution - see https://github.com/kward/shunit2/issues/112
  [[ ! -e test.pid ]] && return 0

  echo "taking down server process $(cat test.pid)..."
  pkill -F test.pid

  rm test.pid
}

test404() {
  curl -s -f -D "$SHUNIT_TMPDIR/headers" -o "$SHUNIT_TMPDIR/out" "http://$FSS_HOST/derp"
  assertEquals 'HTTP/1.1 404 Not Found'$'\r' "$(head -n 1 "$SHUNIT_TMPDIR/headers")"
}

testHello() {
  curl -s -f -D "$SHUNIT_TMPDIR/headers" -o "$SHUNIT_TMPDIR/out" "http://$FSS_HOST/hello"
  assertEquals 'HTTP/1.1 200 OK'$'\r' "$(head -n 1 "$SHUNIT_TMPDIR/headers")"
  assertEquals 'Hello!' "$(cat "$SHUNIT_TMPDIR/out")"
}

source shunit2

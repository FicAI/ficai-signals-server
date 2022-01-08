#!/bin/bash

source test.env
export FICAI_LISTEN FICAI_DB_HOST FICAI_DB_PORT FICAI_DB_USERNAME FICAI_DB_PASSWORD FICAI_DB_DATABASE FICAI_PWD_PEPPER FICAI_DOMAIN

TEST_UID=7357
TEST_URL="https://forums.sufficientvelocity.com/threads/on-fluttering-wings-taylor-hebert-in-mgln.53204/"

request_get() {
  curl -s --fail-with-body -D "$SHUNIT_TMPDIR/headers" -o "$SHUNIT_TMPDIR/out" \
    --cookie "FicAiUid=$TEST_UID" \
    -G --data-urlencode "url=$TEST_URL" \
    "http://$FICAI_LISTEN/v1/signals"
}

build_patch_body() {
  local URL=$1
  shift
  local JSON="{\"url\":\"$URL\"}"
  local DST=""
  local TAG=""
  while [[ $# -gt 0 ]]; do
    DST=""
    case "$1" in
      +*) DST="add";;
      -*) DST="rm";;
      %*) DST="erase";;
      *)
        echo "unexpected patch argument: $1" >&2
        return 1
    esac
    TAG="${1:1}"
    shift
    JSON="$( jq -c ". + {\"$DST\": (.$DST + [\"$TAG\"])}" <<<"$JSON" )"
  done
  echo "$JSON"
}

test_build_patch_body() {
  assertEquals \
    '{"url":"my-url","add":["a","b b","e"],"rm":["a","c"],"erase":["d"]}' \
    "$( build_patch_body my-url +a "+b b" -a -c %d +e )"
}

request_patch() {
  local USER_ID="$1"
  shift
  local JSON="$( build_patch_body "$@" )"
  curl -s --fail-with-body -D "$SHUNIT_TMPDIR/headers" -o "$SHUNIT_TMPDIR/out" \
    --cookie "FicAiUid=$USER_ID" \
    -X PATCH -H "Content-Type: application/json" --data-binary "$JSON" \
    "http://$FICAI_LISTEN/v1/signals"
}

show_headers() {
  cat "$SHUNIT_TMPDIR/headers"
}

show_output() {
  cat "$SHUNIT_TMPDIR/out"
}

show_cookies() {
  cat test.cookies
}

extractSignal() {
  <"$SHUNIT_TMPDIR/out" jq -r ".tags[]|select(.tag==\"$1\")"
}

assertSignal() {
  local TAG="$( extractSignal "$1" )"
  assertEquals "$1" "$2" "$(echo "$TAG" | jq -r '.signal')"
  assertEquals "$1" "$3" "$(echo "$TAG" | jq -r '.signalsFor')"
  assertEquals "$1" "$4" "$(echo "$TAG" | jq -r '.signalsAgainst')"
}

assertNoSignal() {
  assertEquals "$1" "" "$( extractSignal "$1" )"
}

oneTimeSetUp() {
  cargo build || return 1
  nohup "${CARGO_TARGET_DIR:-./target}/debug/ficai-signals-server" >test.log 2>&1 &
  echo $! >test.pid
  echo "server process pid: $(cat test.pid)"
  for i in {1..5} ; do
    sleep 0.1s
    pgrep -F test.pid >/dev/null && break
    [[ "$i" -eq 5 ]] && echo "tired of waiting for server to start" && exit 1
  done
  # todo: wait for server initialization
  sleep 1

  rm -f test.cookies
}

oneTimeTearDown() {
  # shunit2 2.1.8 runs teardown once more at the end of execution - see https://github.com/kward/shunit2/issues/112
  [[ ! -e test.pid ]] && return 0

  echo "taking down server process $(cat test.pid)..."
  pkill -F test.pid

  rm test.pid
}

headers_line() {
  head -n "$1" "$SHUNIT_TMPDIR/headers" | tail -n 1 | tr -d $'\r'
}

test404() {
  curl -s --fail-with-body -D "$SHUNIT_TMPDIR/headers" -o "$SHUNIT_TMPDIR/out" "http://$FICAI_LISTEN/derp"
  assertEquals 'HTTP/1.1 404 Not Found' "$( headers_line 1 )"
}

testCreateUser() {
  curl -s --fail-with-body -D "$SHUNIT_TMPDIR/headers" -o "$SHUNIT_TMPDIR/out" \
    --cookie-jar "test.cookies" \
    -X POST -H "Content-Type: application/json" --data-binary "{\"email\":\"test@example.com\",\"password\":\"pass\"}" \
    "http://$FICAI_LISTEN/v1/account"
  assertEquals 'HTTP/1.1 201 Created' "$( headers_line 1 )"
  assertTrue "cookie must be set" "grep -q FicAiSession test.cookies"
}

testCreateUserSecondTime() {
  curl -s --fail-with-body -D "$SHUNIT_TMPDIR/headers" -o "$SHUNIT_TMPDIR/out" \
    --cookie-jar "test.cookies" \
    -X POST -H "Content-Type: application/json" --data-binary "{\"email\":\"test@example.com\",\"password\":\"pass\"}" \
    "http://$FICAI_LISTEN/v1/account"
  assertEquals 'HTTP/1.1 409 Conflict' "$( headers_line 1 )"
  assertEquals 'account already exists' "$( show_output )"
}

#testGetEmptySignals() {
#  request_get
#  assertEquals 'HTTP/1.1 200 OK' "$( headers_line 1 )"
#  assertEquals '{"tags":[]}' "$(cat "$SHUNIT_TMPDIR/out")"
#}
#
#testAdd() {
#  request_patch $TEST_UID $TEST_URL +worm +taylor
#  request_get
#  assertEquals 'HTTP/1.1 200 OK' "$( headers_line 1 )"
#  assertSignal worm true 1 0
#  assertSignal taylor true 1 0
#}
#
#testRm() {
#  request_patch $TEST_UID $TEST_URL -taylor "+taylor hebert"
#  request_get
#  assertEquals 'HTTP/1.1 200 OK' "$( headers_line 1 )"
#  assertSignal worm true 1 0
#  assertSignal taylor false 0 1
#  assertSignal "taylor hebert" true 1 0
#}
#
#testErase() {
#  request_patch $TEST_UID $TEST_URL %taylor
#  request_get
#  assertEquals 'HTTP/1.1 200 OK' "$( headers_line 1 )"
#  assertSignal worm true 1 0
#  assertSignal "taylor hebert" true 1 0
#  assertNoSignal taylor
#}

source shunit2

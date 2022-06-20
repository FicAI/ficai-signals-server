#!/bin/bash

source test.env
export FICAI_LISTEN FICAI_DB_HOST FICAI_DB_PORT FICAI_DB_USERNAME FICAI_DB_PASSWORD FICAI_DB_DATABASE FICAI_PWD_PEPPER FICAI_DOMAIN FICAI_BETA_KEY FICAI_BEX_LATEST_VERSION FICAI_FICHUB_BASE_URL

TEST_TS="$( date +%s )"
TEST_EMAIL1="${TEST_TS}.1@example.com"
TEST_EMAIL2="${TEST_TS}.2@example.com"
TEST_URL="https://forums.spacebattles.com/threads/nemesis-worm-au.747148/"
TEST_TAG="tag_${TEST_TS}"
TEST_UID="none"

DEBUG=no

request() {
  local CURL_ARGS
  local CURL_RESULT
  if [[ "$DEBUG" == "yes" ]]; then
    CURL_ARGS="-v"
  else
    CURL_ARGS="-s"
  fi
  curl $CURL_ARGS -D "$SHUNIT_TMPDIR/headers" -o "$SHUNIT_TMPDIR/out" \
    --cookie test.cookies --cookie-jar test.cookies \
    "$@"
  CURL_RESULT="$?"
  if [[ "$DEBUG" == "yes" ]]; then
    show_headers
    show_output
    show_cookies
  fi
  # all requests should return json
  assertEquals 'content-type: application/json' "$( grep content-type "$SHUNIT_TMPDIR/headers" | tr -d '\r\n' )"
  assertTrue "invalid json" "cat $SHUNIT_TMPDIR/out | jq >/dev/null"
  return "$CURL_RESULT"
}

request_get() {
  request "http://$FICAI_LISTEN/v1/signals" \
    -G --data-urlencode "url=$TEST_URL"
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
  local JSON="$( build_patch_body "$@" )"
  request "http://$FICAI_LISTEN/v1/signals" \
    -X PATCH -H "Content-Type: application/json" --data-binary "$JSON"
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

assertStatus() {
  assertEquals 'http status' "$1" "$( headers_line 1 )"
}

assertError() {
  assertEquals 'error msg' "$1" "$( show_output | jq -r .error.message )"
}

extractUid() {
  <"$SHUNIT_TMPDIR/out" jq -r ".id"
}

extractEmail() {
  <"$SHUNIT_TMPDIR/out" jq -r ".email"
}

extractSignal() {
  <"$SHUNIT_TMPDIR/out" jq -r ".signals[]|select(.tag==\"$1\")"
}

extractRetired() {
  <"$SHUNIT_TMPDIR/out" jq -r ".retired"
}

extractLatestVersion() {
  <"$SHUNIT_TMPDIR/out" jq -r ".latest_version"
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

extractFirstTag() {
  <"$SHUNIT_TMPDIR/out" jq -r ".tags[0]"
}

extractTag() {
  <"$SHUNIT_TMPDIR/out" jq -r ".tags[]|select(.==\"$1\")"
}

assertTag() {
  assertEquals "tag present" "$1" "$( extractTag "$1" )"
}

assertNoTag() {
  assertEquals "tag not present" "" "$( extractTag "$1" )"
}

extractFicId() {
  <"$SHUNIT_TMPDIR/out" jq -r ".id"
}

extractFicTitle() {
  <"$SHUNIT_TMPDIR/out" jq -r ".title"
}

extractFicSource() {
  <"$SHUNIT_TMPDIR/out" jq -r ".source"
}

oneTimeSetUp() {
  cargo build --example fake_fichub || return 1
  nohup "${CARGO_TARGET_DIR:-./target}/debug/examples/fake_fichub" >fake_fichub.log 2>&1 &
  echo $! >fake_fichub.pid
  echo "fake fichub process pid: $(cat fake_fichub.pid)"

  cargo build || return 1
  nohup "${CARGO_TARGET_DIR:-./target}/debug/ficai-signals-server" >test.log 2>&1 &
  echo $! >test.pid
  echo "server process pid: $(cat test.pid)"
  for i in {1..5} ; do
    sleep 0.1s
    pgrep -F test.pid >/dev/null && break

    [[ "$i" -eq 5 ]] && pkill -F fake_fichub.pid && echo "tired of waiting for server to start" && exit 1
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

  echo "taking down fake fichub process $(cat fake_fichub.pid)..."
  pkill -F fake_fichub.pid
  rm fake_fichub.pid
}

headers_line() {
  head -n "$1" "$SHUNIT_TMPDIR/headers" | tail -n 1 | tr -d $'\r'
}

test404() {
  request "http://$FICAI_LISTEN/derp"
  assertStatus 'HTTP/1.1 404 Not Found'
  assertError 'not found'
}

test405() {
  request "http://$FICAI_LISTEN/v1/signals" -X PUT
  assertStatus 'HTTP/1.1 405 Method Not Allowed'
  assertError 'method not allowed'
}

testUnauthorizedPatch() {
  request_patch "$TEST_URL" +worm +taylor
  assertStatus 'HTTP/1.1 403 Forbidden'
  assertError 'forbidden'
}

testUnauthorizedGetSessionAccount() {
  request "http://$FICAI_LISTEN/v1/sessions"

  assertStatus 'HTTP/1.1 403 Forbidden'
  assertError 'forbidden'
}

testCreateAccountInvalidJSON() {
  request "http://$FICAI_LISTEN/v1/accounts" \
    -X POST -H "Content-Type: application/json" --data-binary "{"
  assertStatus 'HTTP/1.1 400 Bad Request'
  assertError 'bad request body'
}

testCreateAccountInvalidBetaKey() {
  request "http://$FICAI_LISTEN/v1/accounts" \
    -X POST -H "Content-Type: application/json" --data-binary "{\"email\":\"$TEST_EMAIL1\",\"password\":\"pass\",\"betaKey\":\"x$FICAI_BETA_KEY\"}"

  assertStatus 'HTTP/1.1 400 Bad Request'
  assertError 'invalid beta key'
  assertFalse "cookie must not be set" "grep -q FicAiSession test.cookies"
}

testCreateAccount() {
  request "http://$FICAI_LISTEN/v1/accounts" \
    -X POST -H "Content-Type: application/json" --data-binary "{\"email\":\"$TEST_EMAIL1\",\"password\":\"pass\",\"betaKey\":\"$FICAI_BETA_KEY\"}"

  assertStatus 'HTTP/1.1 201 Created'
  assertEquals "$TEST_EMAIL1" "$( extractEmail )"
  assertTrue "cookie must be set" "grep -q FicAiSession test.cookies"
  TEST_UID="$( extractUid )"
}

testCreateAccountSecondTime() {
  request "http://$FICAI_LISTEN/v1/accounts" \
    -X POST -H "Content-Type: application/json" --data-binary "{\"email\":\"$TEST_EMAIL1\",\"password\":\"pass\",\"betaKey\":\"$FICAI_BETA_KEY\"}"
  assertStatus 'HTTP/1.1 409 Conflict'
  assertError 'account already exists'
}

testGetInvalidQuery() {
  request "http://$FICAI_LISTEN/v1/signals" \
    -G --data-urlencode "urlx=$TEST_URL"
  assertStatus 'HTTP/1.1 400 Bad Request'
  assertError 'bad request query'
}

testGetEmptySignals() {
  request_get
  assertStatus 'HTTP/1.1 200 OK'
  assertEquals '{"signals":[]}' "$( show_output )"
}

testAddInvalidJSON() {
  request "http://$FICAI_LISTEN/v1/signals" \
    -X PATCH -H "Content-Type: application/json" --data-binary "{"
  assertStatus 'HTTP/1.1 400 Bad Request'
  assertError 'bad request body'
}

testAdd() {
  request_patch "$TEST_URL" +worm +taylor
  request_get
  assertStatus 'HTTP/1.1 200 OK'
  assertSignal worm true 1 0
  assertSignal taylor true 1 0
}

testGetTagsInvalidQuery() {
  request "http://$FICAI_LISTEN/v1/signals" \
    -G --data-urlencode "limit=five"
  assertStatus 'HTTP/1.1 400 Bad Request'
  assertError 'bad request query'
}

testGetTags() {
  request "http://$FICAI_LISTEN/v1/tags"
  assertStatus 'HTTP/1.1 200 OK'

  assertTag "worm"
  assertNoTag "${TEST_TAG}"

  request_patch "$TEST_URL" "+${TEST_TAG}"
  request "http://$FICAI_LISTEN/v1/tags"
  assertStatus 'HTTP/1.1 200 OK'
  assertTag "${TEST_TAG}"

  request "http://$FICAI_LISTEN/v1/tags" \
    -G --data-urlencode "q=taylor&limit=1"
  assertStatus 'HTTP/1.1 200 OK'
  assertEquals 'taylor' "$( extractFirstTag )"

  request "http://$FICAI_LISTEN/v1/tags" \
    -G --data-urlencode "q=$TEST_TAG&limit=1"
  assertStatus 'HTTP/1.1 200 OK'
  assertEquals "$TEST_TAG" "$( extractFirstTag )"

  request_patch "$TEST_URL" "%${TEST_TAG}"
  request "http://$FICAI_LISTEN/v1/tags"
  assertStatus 'HTTP/1.1 200 OK'
  assertNoTag "${TEST_TAG}"
}

testRm() {
  request_patch "$TEST_URL" -taylor "+taylor hebert"
  request_get
  assertStatus 'HTTP/1.1 200 OK'
  assertSignal worm true 1 0
  assertSignal taylor false 0 1
  assertSignal "taylor hebert" true 1 0
}

testCreateSessionInvalidJSON() {
  request "http://$FICAI_LISTEN/v1/sessions" \
    -X POST -H "Content-Type: application/json" --data-binary "{"
  assertStatus 'HTTP/1.1 400 Bad Request'
  assertError 'bad request body'
}

testGetSessionAccount() {
  request "http://$FICAI_LISTEN/v1/sessions"

  assertStatus 'HTTP/1.1 200 OK'
  assertTrue "cookie must be set" "grep -q FicAiSession test.cookies"
  assertEquals "$TEST_EMAIL1" "$( extractEmail )"
  assertEquals "$TEST_UID" "$( extractUid )"
}

testDeleteSession() {
  assertTrue "cookie must be set" "grep -q FicAiSession test.cookies"
  request "http://$FICAI_LISTEN/v1/sessions" -X DELETE

  assertStatus 'HTTP/1.1 200 OK'
  assertEquals "{}" "$( show_output )"
  assertFalse "cookie must not be set" "grep -q FicAiSession test.cookies"
}

testDeleteSessionSecondTime() {
  assertFalse "cookie must not be set" "grep -q FicAiSession test.cookies"
  request "http://$FICAI_LISTEN/v1/sessions" -X DELETE

  assertStatus 'HTTP/1.1 403 Forbidden'
  assertError 'forbidden'
  assertFalse "cookie must not be set" "grep -q FicAiSession test.cookies"
}

testUnauthorizedGetSignals() {
  request_get
  assertStatus 'HTTP/1.1 200 OK'
  assertSignal worm null 1 0
  assertSignal "taylor hebert" null 1 0
  assertSignal taylor null 0 1
}

testCreateSession() {
  rm test.cookies
  request "http://$FICAI_LISTEN/v1/sessions" \
    -X POST -H "Content-Type: application/json" --data-binary "{\"email\":\"$TEST_EMAIL1\",\"password\":\"pass\"}"

  assertStatus 'HTTP/1.1 200 OK'
  assertEquals "$TEST_EMAIL1" "$( extractEmail )"
  assertEquals "$TEST_UID" "$( extractUid )"
  assertTrue "cookie must be set" "grep -q FicAiSession test.cookies"
}

testGetSignals() {
  request_get
  assertStatus 'HTTP/1.1 200 OK'
  assertSignal worm true 1 0
  assertSignal "taylor hebert" true 1 0
  assertSignal taylor false 0 1
}

testErase() {
  request_patch "$TEST_URL" %taylor
  request_get
  assertStatus 'HTTP/1.1 200 OK'
  assertSignal worm true 1 0
  assertSignal "taylor hebert" true 1 0
  assertNoSignal taylor
}

testErase2() {
  request_patch "$TEST_URL" %worm '%taylor hebert'
  request_get
  assertStatus 'HTTP/1.1 200 OK'
  assertNoSignal worm
  assertNoSignal "taylor hebert"
  assertNoSignal taylor
}

testCreateSessionWithWrongEmail() {
  request "http://$FICAI_LISTEN/v1/sessions" \
    -X POST -H "Content-Type: application/json" --data-binary "{\"email\":\"$TEST_EMAIL2\",\"password\":\"pass\"}"

  assertStatus 'HTTP/1.1 403 Forbidden'
  assertError 'forbidden'
}

testCreateSessionWithWrongPassword() {
  request "http://$FICAI_LISTEN/v1/sessions" \
    -X POST -H "Content-Type: application/json" --data-binary "{\"email\":\"$TEST_EMAIL1\",\"password\":\"wrong pass\"}"

  assertStatus 'HTTP/1.1 403 Forbidden'
  assertError 'forbidden'
}

testUnauthorizedGetSignalsEmpty() {
  assertTrue "cookie must be set" "grep -q FicAiSession test.cookies"
  request "http://$FICAI_LISTEN/v1/sessions" -X DELETE

  assertStatus 'HTTP/1.1 200 OK'
  assertEquals "{}" "$( show_output )"
  assertFalse "cookie must not be set" "grep -q FicAiSession test.cookies"

  request_get
  assertStatus 'HTTP/1.1 200 OK'
  assertNoSignal worm
  assertNoSignal "taylor hebert"
  assertNoSignal taylor
}

testGetBex() {
  request "http://$FICAI_LISTEN/v1/bex/versions/v0.1.0"
  assertStatus 'HTTP/1.1 200 OK'
  assertEquals false "$( extractRetired )"
  assertEquals "${FICAI_BEX_LATEST_VERSION}" "$( extractLatestVersion )"

  request "http://$FICAI_LISTEN/v1/bex/versions/v0.0.0"
  assertStatus 'HTTP/1.1 200 OK'
  assertEquals true "$( extractRetired )"
  assertEquals "${FICAI_BEX_LATEST_VERSION}" "$( extractLatestVersion )"
}

testGetFicInvalidQuery() {
  request "http://$FICAI_LISTEN/v1/fics" \
    -G --data-urlencode "urlz=${TEST_URL}"
  assertStatus 'HTTP/1.1 400 Bad Request'
  assertError 'bad request query'
}

testGetFic() {
  request "http://$FICAI_LISTEN/v1/fics" \
    -G --data-urlencode "url=${TEST_URL}"
  assertStatus 'HTTP/1.1 200 OK'
  assertEquals 'NtePoQrV' "$( extractFicId )"
  assertEquals 'Nemesis' "$( extractFicTitle )"
  assertEquals "${TEST_URL}" "$( extractFicSource )"
}

testGetFicError() {
  request "http://$FICAI_LISTEN/v1/fics" \
    -G --data-urlencode "url=not-a-fic"
  assertStatus 'HTTP/1.1 500 Internal Server Error'
  assertError 'failed to query fic metadata'
}

testGetFicTimeout() {
  request "http://$FICAI_LISTEN/v1/fics" \
    -G --data-urlencode "url=hang-15"
  assertStatus 'HTTP/1.1 500 Internal Server Error'
  assertError 'failed to query fic metadata'
}

source shunit2

#!/usr/bin/env bash
# End-to-end protocol verification against local mock servers.
# Usage: scripts/protocol-e2e.sh
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
E2E=/tmp/ariaflow-e2e
DB="$E2E/ariaflow.db"
OUT="$E2E/out"
API=127.0.0.1:18999
TOKEN=e2e-token
DAEMON_BIN="$ROOT/target/debug/ariaflowd"
PAYLOAD_DIR="$E2E/payload"

export PAYLOAD_DIR HTTP_PORT=18901 THROTTLE_PORT=18904 FTP_PORT=18902 SFTP_PORT=18903
mkdir -p "$E2E" "$OUT"
rm -f "$DB"
rm -rf "$OUT" && mkdir -p "$OUT"

pass=0; fail=0
ok()  { echo "  PASS: $1"; pass=$((pass+1)); }
bad() { echo "  FAIL: $1"; fail=$((fail+1)); }

echo "== building daemon =="
if ! cargo build -p ariaflow-daemon 2>&1 | tail -20; then
  echo "daemon build failed"; exit 1
fi
[ -x "$DAEMON_BIN" ] || { echo "daemon binary missing after build: $DAEMON_BIN"; exit 1; }

echo "== starting mock servers =="
python3 "$ROOT/scripts/mock_servers.py" > "$E2E/servers.log" 2>&1 &
SERVERS_PID=$!
for i in $(seq 1 20); do grep -q "^http=" "$E2E/servers.log" 2>/dev/null && break; sleep 0.25; done
cat "$E2E/servers.log"; EXPECTED_DIGEST=$(sed -n "s/.*sha256=//p" "$E2E/servers.log")
[ -n "$EXPECTED_DIGEST" ] || { echo "mock servers failed to start"; kill $SERVERS_PID; exit 1; }

echo "== starting ariaflowd (db: $DB) =="
ARIAFLOW_DB="$DB" ARIAFLOW_LOG=warn "$DAEMON_BIN" --api-bind "$API" --token "$TOKEN" > "$E2E/daemon.log" 2>&1 &
DAEMON_PID=$!
for i in $(seq 1 40); do curl -fsS -m 2 -H "Authorization: Bearer $TOKEN" "http://$API/api/v1/status" > /dev/null 2>&1 && break; sleep 0.25; done

add_download() { # $1=json body ; echoes task id or dies with the server response
  local resp code body
  resp=$(curl -sS -m 10 -w '\n%{http_code}' -X POST -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
    -d "$1" "http://$API/api/v1/downloads")
  code=$(printf '%s' "$resp" | tail -n1)
  body=$(printf '%s' "$resp" | sed '$d')
  if [ "$code" != 201 ]; then
    echo "add_download failed ($code): $body" >&2
    return 1
  fi
  printf '%s' "$body" | python3 -c 'import sys,json;print(json.load(sys.stdin)["id"])'
}

wait_status() { # $1=id $2=wanted-status ; echoes final status
  local id="$1" want="$2" status=""
  for i in $(seq 1 60); do
    status=$(curl -fsS -m 5 -H "Authorization: Bearer $TOKEN" "http://$API/api/v1/downloads" \
      | python3 -c "import sys,json;items=json.load(sys.stdin);items=items if isinstance(items,list) else items.get('items',items.get('downloads',[]));d=next((x for x in items if x['id']=='$id'),None);print(d['status'] if d else 'missing')")
    case "$status" in
      "$want"|completed|failed|cancelled) echo "$status"; return 0;;
    esac
    sleep 0.5
  done
  echo "timeout($status)"
}

check_file() { # $1=expected path ; compares with payload
  cmp -s "$PAYLOAD_DIR/payload.bin" "$1"
}

echo
echo "== T1: HTTP download =="
URL="http://127.0.0.1:18901/payload.bin?t1=1"
ID=$(add_download "{\"url\":\"$URL\",\"filename\":\"t1.bin\",\"saveDir\":\"$OUT\"}")
S=$(wait_status "$ID" completed)
[ "$S" = completed ] && check_file "$OUT/t1.bin" && ok "HTTP download completed and bytes match" || bad "T1 status=$S"

echo "== T2: HTTP with expected checksum (correct) =="
URL="http://127.0.0.1:18901/payload.bin?t2=1"
ID=$(add_download "{\"url\":\"$URL\",\"filename\":\"t2.bin\",\"saveDir\":\"$OUT\",\"expectedChecksum\":{\"algorithm\":\"sha256\",\"digest\":\"$EXPECTED_DIGEST\"},\"maxRetries\":0}")
S=$(wait_status "$ID" completed)
[ "$S" = completed ] && ok "checksum-verified download completed" || bad "T2 status=$S"

echo "== T3: HTTP with expected checksum (wrong digest, maxRetries=0) =="
URL="http://127.0.0.1:18901/payload.bin?t3=1"
WRONG=$(printf '0%.0s' {1..64})
ID=$(add_download "{\"url\":\"$URL\",\"filename\":\"t3.bin\",\"saveDir\":\"$OUT\",\"expectedChecksum\":{\"algorithm\":\"sha256\",\"digest\":\"$WRONG\"},\"maxRetries\":0}")
S=$(wait_status "$ID" failed)
[ "$S" = failed ] && ok "checksum mismatch correctly fails the task (maxRetries=0)" || bad "T3 status=$S"

echo "== T4: FTP (anonymous) =="
ID=$(add_download "{\"url\":\"ftp://anonymous:anonymous@127.0.0.1:18902/payload.bin\",\"filename\":\"t4.bin\",\"saveDir\":\"$OUT\",\"maxRetries\":0}")
S=$(wait_status "$ID" completed)
[ "$S" = completed ] && check_file "$OUT/t4.bin" && ok "FTP download completed and bytes match" || bad "T4 status=$S"

echo "== T5: SFTP (password auth) =="
ID=$(add_download "{\"url\":\"sftp://ariaflow:s3cret@127.0.0.1:18903/payload.bin\",\"filename\":\"t5.bin\",\"saveDir\":\"$OUT\",\"maxRetries\":0}")
S=$(wait_status "$ID" completed)
[ "$S" = completed ] && check_file "$OUT/t5.bin" && ok "SFTP download completed and bytes match" || bad "T5 status=$S"

echo "== T6: Metalink (mirror + sha256 manifest) =="
ID=$(add_download "{\"url\":\"metalink:http://127.0.0.1:18901/files.metalink\",\"filename\":\"t6.bin\",\"saveDir\":\"$OUT\",\"maxRetries\":0}")
S=$(wait_status "$ID" completed)
[ "$S" = completed ] && check_file "$OUT/t6.bin" && ok "metalink download completed and bytes match" || bad "T6 status=$S"

echo "== T7: pause/resume across .ariaflow-part =="
ID=$(add_download "{\"url\":\"http://127.0.0.1:18904/payload.bin\",\"filename\":\"t7.bin\",\"saveDir\":\"$OUT\",\"maxRetries\":0}")
sleep 1.2
curl -fsS -m 5 -X POST -H "Authorization: Bearer $TOKEN" "http://$API/api/v1/downloads/$ID/pause" > /dev/null || bad "T7 pause request failed"
PART="$OUT/t7.bin.ariaflow-part"
# Segmented transfers keep bytes in .ariaflow-part.seg-NNN side files instead.
for i in $(seq 1 20); do
  if [ -f "$PART" ] || compgen -G "$PART.seg-*" > /dev/null; then break; fi
  sleep 0.25
done
if [ -f "$PART" ]; then
  ok "partial .ariaflow-part exists after pause ($(stat -c%s "$PART") bytes)"
elif compgen -G "$PART.seg-*" > /dev/null; then
  ok "segment side files exist after pause ($(cat $PART.seg-* | wc -c) bytes total)"
else
  bad "T7 no partial/segment files after pause"
fi
S=$(wait_status "$ID" paused)
[ "$S" = paused ] && ok "task reached paused state" || bad "T7 pause status=$S"
curl -fsS -m 5 -X POST -H "Authorization: Bearer $TOKEN" "http://$API/api/v1/downloads/$ID/resume" > /dev/null || bad "T7 resume request failed"
S=$(wait_status "$ID" completed)
[ "$S" = completed ] && check_file "$OUT/t7.bin" && ok "resumed download completed and bytes match" || bad "T7 after-resume status=$S"

echo "== T8: probe endpoint (http/ftp/sftp) =="
for url in "http://127.0.0.1:18901/payload.bin" "ftp://anonymous@127.0.0.1:18902/payload.bin" "sftp://ariaflow:s3cret@127.0.0.1:18903/payload.bin"; do
  code=$(curl -sS -m 10 -o /tmp/ariaflow-e2e/probe.json -w "%{http_code}" -X POST -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" -d "{\"url\":\"$url\"}" "http://$API/api/v1/probe")
  if [ "$code" = 200 ] && python3 -c "import json;d=json.load(open('/tmp/ariaflow-e2e/probe.json'));assert d.get('totalSize')==2097152 or (d.get('totalSize') or 0)>0" 2>/dev/null; then
    ok "probe $url"
  else
    bad "probe $url (http=$code, body=$(head -c 200 /tmp/ariaflow-e2e/probe.json))"
  fi
done

echo "== T9: segmented download (maxConnections=4, small segments) =="
curl -fsS -m 5 -X PUT -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"minSegmentSize":65536}' "http://$API/api/v1/settings" > /dev/null || bad "T9 settings update failed"
ID9=$(add_download "{\"url\":\"http://127.0.0.1:18901/payload.bin\",\"filename\":\"t9.bin\",\"saveDir\":\"$OUT\",\"maxConnections\":4}")
S9=$(wait_status "$ID9" completed)
[ "$S9" = completed ] && check_file "$OUT/t9.bin" && ok "segmented download completed and bytes match" || bad "T9 segmented status=$S9"
curl -fsS -m 5 -X PUT -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"minSegmentSize":1048576}' "http://$API/api/v1/settings" > /dev/null || true

echo "== T10: segmented + checksum end-to-end (maxConnections=4) =="
curl -fsS -m 5 -X PUT -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"minSegmentSize":65536}' "http://$API/api/v1/settings" > /dev/null || bad "T10 settings update failed"
ID10=$(add_download "{\"url\":\"http://127.0.0.1:18901/payload.bin\",\"filename\":\"t10.bin\",\"saveDir\":\"$OUT\",\"maxConnections\":4,\"expectedChecksum\":{\"algorithm\":\"sha256\",\"digest\":\"$EXPECTED_DIGEST\"}}")
S10=$(wait_status "$ID10" completed)
[ "$S10" = completed ] && check_file "$OUT/t10.bin" && ok "segmented + checksum-verified download completed" || bad "T10 segmented+checksum status=$S10"
curl -fsS -m 5 -X PUT -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"minSegmentSize":1048576}' "http://$API/api/v1/settings" > /dev/null || true

echo "== T11: scheduled start (future) holds task in queued =="
FUTURE=$(python3 -c "import datetime;print((datetime.datetime.now(datetime.timezone.utc)+datetime.timedelta(hours=1)).strftime('%Y-%m-%dT%H:%M:%SZ'))")
ID11=$(add_download "{\"url\":\"http://127.0.0.1:18901/payload.bin?t11=1\",\"filename\":\"t11.bin\",\"saveDir\":\"$OUT\",\"scheduledStartAt\":\"$FUTURE\"}")
sleep 2
S11=$(curl -fsS -m 5 -H "Authorization: Bearer $TOKEN" "http://$API/api/v1/downloads" \
  | python3 -c "import sys,json;items=json.load(sys.stdin);items=items if isinstance(items,list) else items.get('items',items.get('downloads',[]));d=next((x for x in items if x['id']=='$ID11'),None);print(d['status'] if d else 'missing')")
if [ "$S11" = queued ]; then ok "scheduled task stays queued until its start instant"; else bad "T11 status=$S11 (expected queued)"; fi
curl -fsS -m 5 -X POST -H "Authorization: Bearer $TOKEN" "http://$API/api/v1/downloads/$ID11/cancel" > /dev/null || bad "T11 cancel request failed"
S11C=$(wait_status "$ID11" cancelled)
[ "$S11C" = cancelled ] && ok "scheduled task cancellable while waiting" || bad "T11 cancel status=$S11C"

echo "== T12: daily window outside current time defers automatic start =="
read WS WE <<<$(python3 -c "
import datetime
now = datetime.datetime.now()
cur = now.hour * 60 + now.minute
print((cur + 120) % 1440, (cur + 180) % 1440)")
curl -fsS -m 5 -X PUT -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d "{\"downloadWindowStartMinute\":$WS,\"downloadWindowEndMinute\":$WE}" "http://$API/api/v1/settings" > /dev/null || bad "T12 settings update failed"
ID12=$(add_download "{\"url\":\"http://127.0.0.1:18901/payload.bin?t12=1\",\"filename\":\"t12.bin\",\"saveDir\":\"$OUT\"}")
sleep 2
S12=$(curl -fsS -m 5 -H "Authorization: Bearer $TOKEN" "http://$API/api/v1/downloads" \
  | python3 -c "import sys,json;items=json.load(sys.stdin);items=items if isinstance(items,list) else items.get('items',items.get('downloads',[]));d=next((x for x in items if x['id']=='$ID12'),None);print(d['status'] if d else 'missing')")
if [ "$S12" = queued ]; then ok "outside the download window the task stays queued"; else bad "T12 status=$S12 (expected queued)"; fi
curl -fsS -m 5 -X PUT -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"downloadWindowStartMinute":null,"downloadWindowEndMinute":null}' "http://$API/api/v1/settings" > /dev/null || bad "T12 settings clear failed"
S12C=$(wait_status "$ID12" completed)
[ "$S12C" = completed ] && check_file "$OUT/t12.bin" && ok "after clearing the window the queued task starts and completes" || bad "T12 completion status=$S12C"

echo "== T13: global speed limit paces downloads =="
curl -fsS -m 5 -X PUT -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"globalSpeedLimit":524288}' "http://$API/api/v1/settings" > /dev/null || bad "T13 settings update failed"
T13_START=$(python3 -c 'import time;print(time.time())')
ID13=$(add_download "{\"url\":\"http://127.0.0.1:18901/payload.bin?t13=1\",\"filename\":\"t13.bin\",\"saveDir\":\"$OUT\"}")
S13=$(wait_status "$ID13" completed)
T13_END=$(python3 -c 'import time;print(time.time())')
python3 -c "import sys; d=$T13_END-$T13_START; print(f'  elapsed: {d:.1f}s'); sys.exit(0 if d >= 2.0 else 1)" \
  && ok "global 512 KiB/s limit enforced (took >= 2s for 2 MiB)" || bad "T13 finished too fast; limit not enforced"
[ "$S13" = completed ] && check_file "$OUT/t13.bin" && ok "T13 bytes match" || bad "T13 status=$S13"
curl -fsS -m 5 -X PUT -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"globalSpeedLimit":0}' "http://$API/api/v1/settings" > /dev/null || bad "T13 settings restore failed"

echo "== T14: per-task speed limit paces one download =="
T14_START=$(python3 -c 'import time;print(time.time())')
ID14=$(add_download "{\"url\":\"http://127.0.0.1:18901/payload.bin?t14=1\",\"filename\":\"t14.bin\",\"saveDir\":\"$OUT\",\"limitRateBytesPerSec\":524288}")
S14=$(wait_status "$ID14" completed)
T14_END=$(python3 -c 'import time;print(time.time())')
python3 -c "import sys; d=$T14_END-$T14_START; print(f'  elapsed: {d:.1f}s'); sys.exit(0 if d >= 2.0 else 1)" \
  && ok "per-task 512 KiB/s limit enforced (took >= 2s for 2 MiB)" || bad "T14 finished too fast; limit not enforced"
[ "$S14" = completed ] && check_file "$OUT/t14.bin" && ok "T14 bytes match" || bad "T14 status=$S14"

echo
echo "======================================="
echo "protocol e2e: $pass passed, $fail failed"
echo "daemon log tail:"; tail -5 "$E2E/daemon.log"

kill $DAEMON_PID 2>/dev/null; wait $DAEMON_PID 2>/dev/null
kill $SERVERS_PID 2>/dev/null; wait $SERVERS_PID 2>/dev/null
[ $fail -eq 0 ]

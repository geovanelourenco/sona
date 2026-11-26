#!/usr/bin/env bash
# Teste de integração com o libobs real (sem interface): bridge envia um tom ao
# servidor, a Transmissão volta, o bridge entrega ao plugin "Sona" e o plugin
# ao pipeline de áudio do OBS. Requer libobs-dev e xvfb.
#   cargo build --release -p sona-server -p sona-bridge -p sona-obs && tests/obs/run.sh
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
work="$(mktemp -d)"
trap 'kill $(jobs -p) 2>/dev/null || true; rm -rf "$work"' EXIT
cc -I/usr/include/obs "$root/tests/obs/harness.c" -lobs -lm -o "$work/harness"
mkdir -p "$work/srv"
echo '{"primary":"internal"}' > "$work/srv/audio.json"
"$root/target/release/sona-server" --data "$work/srv" --port 7348 > "$work/srv.log" 2>&1 &
sleep 1
"$root/target/release/sona-bridge" --data "$work/br" --port 7354 --server 127.0.0.1:7348 --name OBS --send sim --receive obs > "$work/br.log" 2>&1 &
sleep 3
seconds=8
out="$(xvfb-run -a "$work/harness" "$root/target/release/libsona_obs.so" "$seconds" 2>"$work/obs.log" | grep "^{" | tail -1)"
echo "libobs: $out"
python3 - "$out" "$seconds" <<'PY'
import json, sys
r = json.loads(sys.argv[1]); s = int(sys.argv[2])
assert r.get("nome") == "Sona (transmissão)", r
assert abs(r["quadros"] - 48000 * s) < 48000 * s * 0.05, f"quadros {r['quadros']}"
assert 0.2 < r["pico"] < 0.3, f"pico {r['pico']} (esperado 0,25: tom 0,5 com fader -6 dB)"
print("ok   OBS recebe a transmissão pelo plugin Sona")
PY

#!/bin/sh
# Mede o daemon parado no Linux: memória e CPU depois de N minutos.
# Uso: tools/perf/idle.sh <executável> <minutos>
# Use um config de teste com `autostart = false` (ELGIN_TRAY_CONFIG) para não registrar o
# executável de teste no início da sessão. O daemon é encerrado no fim.
set -eu
exe=$1
minutes=$2
ticks=$(getconf CLK_TCK)

cpu_ms() { awk -v t="$ticks" '{ print ($14 + $15) * 1000 / t }' "/proc/$1/stat"; }
memory() {
    awk '/^(Rss|Pss|Private_Clean|Private_Dirty):/ { printf "%s %d kB  ", $1, $2 }' "/proc/$1/smaps_rollup"
    awk '/^(VmHWM|Threads):/ { printf "%s %s  ", $1, $2 }' "/proc/$1/status"
    printf "FDs: %s\n" "$(ls "/proc/$1/fd" | wc -l)"
}

"$exe" >/dev/null 2>&1 &
pid=$!
trap 'kill $pid 2>/dev/null' EXIT
sleep 30
echo "== depois de 30 s"
memory $pid
start=$(cpu_ms $pid)
sleep $((minutes * 60))
end=$(cpu_ms $pid)
echo "== depois de mais $minutes min"
memory $pid
echo "CPU nesses $minutes min: $(awk -v a="$start" -v b="$end" 'BEGIN { print b - a }') ms"

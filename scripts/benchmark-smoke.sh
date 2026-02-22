#!/usr/bin/env bash
set -euo pipefail

iterations="${1:-25}"
max_median_ms="${BERTH_STARTUP_MAX_MS:-250}"

if ! [[ "$iterations" =~ ^[0-9]+$ ]] || [[ "$iterations" -lt 3 ]]; then
  echo "iterations must be an integer >= 3" >&2
  exit 2
fi

echo "Building release berth binary for benchmark smoke check..."
cargo build --release --package berth-cli --bin berth >/dev/null

bin_path="target/release/berth"
if [[ ! -x "$bin_path" ]]; then
  echo "missing benchmark binary at $bin_path" >&2
  exit 2
fi

durations_ms=()
for ((i = 0; i < iterations; i++)); do
  start_ns="$(date +%s%N)"
  "$bin_path" --version >/dev/null
  end_ns="$(date +%s%N)"
  elapsed_ms="$(((end_ns - start_ns) / 1000000))"
  durations_ms+=("$elapsed_ms")
done

mapfile -t sorted_ms < <(printf "%s\n" "${durations_ms[@]}" | sort -n)
median_index="$((iterations / 2))"
p95_index="$((((iterations * 95) + 99) / 100 - 1))"
if [[ "$p95_index" -ge "$iterations" ]]; then
  p95_index="$((iterations - 1))"
fi

median_ms="${sorted_ms[$median_index]}"
p95_ms="${sorted_ms[$p95_index]}"

echo "Benchmark smoke results:"
echo "  iterations: $iterations"
echo "  median_ms: $median_ms"
echo "  p95_ms: $p95_ms"
echo "  threshold_median_ms: $max_median_ms"

if [[ "$median_ms" -gt "$max_median_ms" ]]; then
  echo "median startup regression detected: ${median_ms}ms > ${max_median_ms}ms" >&2
  exit 1
fi


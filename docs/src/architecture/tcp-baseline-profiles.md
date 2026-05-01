# TCP Baseline Profiles

This document defines the first reproducible Phase 0 baseline runs for the
current TCP media path. Use these before implementing or comparing RTP/UDP.

The goal is to capture measurable TCP behavior under known conditions:

```text
transport=tcp
clean LAN
high jitter
packet loss
burst loss
CPU stress
```

Do not treat these as final performance gates yet. They are the minimum
repeatable profiles needed to compare future transport work honestly.

---

## Metrics to capture

Capture `/metrics` before, during, and after each run.

Required metrics:

```text
sonium_client_health_reports_total{transport="tcp"}
sonium_client_health_state{transport="tcp"}
sonium_client_buffer_depth_ms{transport="tcp"}
sonium_client_output_buffer_ms{transport="tcp"}
sonium_client_jitter_buffer_chunks{transport="tcp"}
sonium_client_target_playout_latency_ms{transport="tcp"}
sonium_client_callback_starvations{transport="tcp"}
sonium_client_audio_callback_xruns{transport="tcp"}
sonium_client_jitter_ms{transport="tcp"}
sonium_client_underruns{transport="tcp"}
sonium_client_stale_drops{transport="tcp"}
sonium_client_overruns{transport="tcp"}
sonium_client_clock_offset_ms{transport="tcp"}
```

Suggested capture loop:

```bash
mkdir -p run/baselines/tcp-clean
while true; do
  ts="$(date +%s)"
  curl -fsS http://127.0.0.1:1711/metrics > "run/baselines/tcp-clean/$ts.prom"
  sleep 5
done
```

Or use the helper script:

```bash
scripts/capture_tcp_baseline.sh \
  --profile tcp-clean \
  --duration 600 \
  --interval 5 \
  --note "one wired client, buffer_ms=500, chunk_ms=20"
```

Also save client and server logs for the same time window.

---

## Common setup

Use a fixed stream configuration for every profile in a comparison set:

```toml
[server]
buffer_ms = 500
chunk_ms = 20
auto_buffer = false
```

Recommended first runs:

```text
duration: 10 minutes per profile
clients: 1, then 2
codec: keep the configured default
transport: tcp only
observability: enabled for every test client
```

If a profile causes immediate failures, keep the result and rerun once with a
larger `buffer_ms` so the failure threshold is visible.

---

## Profile A - clean TCP

Purpose:

```text
Measure normal TCP behavior with no intentional network impairment.
```

Procedure:

1. Start the server with the common setup.
2. Start one client on the same wired LAN.
3. Enable client observability in the admin UI or API.
4. Capture metrics for 10 minutes.
5. Repeat with two clients.

Expected result:

```text
health state should remain stable after initial buffering
underruns should remain flat
stale drops should remain flat
jitter should stay low and bounded
```

---

## Profile B - high jitter

Purpose:

```text
Show whether scheduling or network jitter causes underruns before packet loss is introduced.
```

Linux impairment example:

```bash
sudo tc qdisc add dev eth0 root netem delay 20ms 10ms distribution normal
```

Cleanup:

```bash
sudo tc qdisc del dev eth0 root
```

Procedure:

1. Apply the jitter rule on the client-side test interface or a Linux bridge/router between server and client.
2. Run the same 10-minute capture as Profile A.
3. Record the exact interface, rule, and cleanup command used.

Expected result:

```text
jitter rises
buffer depth may oscillate
underruns should identify the buffer threshold where TCP becomes unstable
```

---

## Profile C - random packet loss

Purpose:

```text
Document TCP recovery behavior under retransmission pressure.
```

Linux impairment examples:

```bash
sudo tc qdisc add dev eth0 root netem loss 1%
sudo tc qdisc change dev eth0 root netem loss 3%
```

Cleanup:

```bash
sudo tc qdisc del dev eth0 root
```

Procedure:

1. Run 10 minutes at 1% loss.
2. Run 10 minutes at 3% loss.
3. Save metrics and logs separately for each loss level.

Expected result:

```text
TCP may avoid packet loss at the application layer but still show stalls
underruns, stale drops, and output buffer collapse are the important signals
```

---

## Profile D - burst loss

Purpose:

```text
Reproduce short freeze behavior from bursty links.
```

Linux impairment example:

```bash
sudo tc qdisc add dev eth0 root netem loss 8% 25%
```

Cleanup:

```bash
sudo tc qdisc del dev eth0 root
```

Procedure:

1. Run at least 10 minutes.
2. Mark audible freezes with wall-clock timestamps.
3. Compare those timestamps against health transitions and output buffer depth.

Expected result:

```text
health transitions should explain the audible event
output/player buffer depth should collapse near audible freezes
```

---

## Profile E - CPU stress

Purpose:

```text
Separate network behavior from local scheduling starvation.
```

Example:

```bash
stress-ng --cpu 2 --io 1 --vm 1 --timeout 10m
```

Procedure:

1. Run clean TCP while applying CPU stress on the client host.
2. Repeat once with stress on the server host.
3. Capture metrics and logs.

Expected result:

```text
if jitter is low but underruns rise, suspect local scheduling or callback starvation
if jitter and stale drops rise together, suspect network/decode queue pressure
callback starvation or xrun counters identify local audio scheduling/device pressure
```

---

## Result notes

For each run, record:

```text
profile name
date/time
server commit or build
client commit or build
client count
codec
buffer_ms
chunk_ms
auto_buffer
network impairment command
audible dropout timestamps
metrics folder
log files
summary of health transitions
```

The accepted TCP baseline should be linked from the transport migration plan
before Phase 1 or Phase 2 work is considered complete.

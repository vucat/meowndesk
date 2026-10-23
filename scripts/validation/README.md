# External validation harnesses

Run from the repository root with Python 3.10+ (standard library only). Use a
release binary built without `test-fixtures`. A native graphical session and
`tauri-driver` plus WebKitWebDriver/msedgedriver are needed for UI actions.
The harness launches its own app/driver and closes only that session. `--pid`
attaches to an existing app tree and leaves it running; UI actions are unmeasured
in that mode. Do not pass an unrelated browser or driver PID as the app root.
Each output directory is a separate evidence run; existing reports are preserved.

```sh
python3 -m unittest discover -s scripts/validation -p 'test_*.py'
python3 scripts/validation/profile.py --binary src-tauri/target/release/meowndesk --driver tauri-driver
python3 scripts/validation/soak.py --binary src-tauri/target/release/meowndesk --driver tauri-driver
# Smoke only. These commands do not establish performance acceptance.
python3 scripts/validation/profile.py --binary src-tauri/target/release/meowndesk --driver tauri-driver --warmup 1 --duration 15 --sample-interval 1 --trials 1 --output artifacts/validation/profile-smoke
python3 scripts/validation/soak.py --binary src-tauri/target/release/meowndesk --driver tauri-driver --warmup 1 --duration 15 --sample-interval 1 --control-interval 4 --output artifacts/validation/soak-smoke
```

On Windows use `python` and the `.exe` executable. `--interval-ms` must match the
app's collection preference (default 1000); use Settings before measuring.
The protocol requires one visible Overview, 1m range, default interval, no
devtools, no dev server, and no fixture provider. The profile interleaves 20
trials each of range, Pause, Resume, Settings-open and Settings-close; soak
cycles range/Pause/minimize/restore/Resume every five minutes. Unsupported
minimize commands are failures recorded with the run, never silently ignored.
The observer polls the visible footer each second, excluding pause/hide and
reporting its polling resolution. It cannot detect every sub-second gap.

Linux sums root/descendant RSS from `/proc`, with readable PSS supplementary.
Windows uses non-elevated CIM process creation IDs, CPU times and working sets.
Observed helpers remain attributed if reparented; PID reuse is excluded. Helpers
never seen under app ancestry need independent ownership review. Lifetime CPU
of newly observed helpers is included; CPU after an exited helper's last sample
cannot be recovered. Python and WebDriver/trace/CIM tooling are outside the app
root. Python CPU time is reported separately; total instrumentation overhead
is unmeasured. No process command lines are collected. Resource means/p95/peaks
and whole-machine CPU (app CPU / elapsed wall / logical CPUs) are descriptive;
60 MB and 1% remain optimization goals.

Supply `--metadata metadata.json` with OS patch, webview/driver versions, GPU IDs,
reference hardware, scaling/display refresh, logical CPU count, memory, and build
provenance. Lockfile and executable hashes are captured automatically. Missing
reference metadata makes the run provisional. Windows CIM and native Windows
trace collection require execution on Windows; Linux smoke is not their proof.

## Timing and native traces

DOM acknowledgement followed by a next-rAF callback is reported as a provisional
visible-feedback proxy. Twenty trials per action get independent nearest-rank
p95 (19th of 20) and a failing exit code if any range/Pause/Resume proxy p95
exceeds 200 ms. This is not native input-to-presentation proof, nor a frame
duration. Settings results are separate. An incomplete sample count stays pending.

`--trace-config config` enables platform capture if supported: Linux invokes
`perfetto --txt -c config -o native.pftrace`; Windows invokes
`wpr -start config.wprp -filemode` and `wpr -stop native.etl`. Provide a native
webview/compositor provider configuration appropriate to that host, with
frame-begin and render-submission events. No elevation is requested. Missing
tools/configuration leave instrumentation **unmeasured**; collection errors fail
the run. Windows WPR sessions should be started only on a dedicated validation
host with no other active WPR recording.

Use the platform trace processor (Perfetto trace processor on Linux; WPA/export
on Windows) to normalize events into the schema below and pass `--frame-trace`.
The producer must establish native event boundaries, app-tree attribution and
clock correlation to `controls.jsonl`'s input times. Capturing a raw trace alone
does not establish these. Idle time is excluded: each frame duration is native
frame-begin through render submission. Only frames beginning 0–500 ms after
an input, with matching action/trial, count. Reports retain source, boundaries,
per-action sample count, covered trials, nearest-rank p95 and max; 20 covered
trials per category are required for complete frame evidence.

```json
{
  "source": "native provider and exported trace filename",
  "platform": "Linux",
  "beginEvent": "actual native frame-begin event name",
  "renderSubmissionEvent": "actual native render-submission event name",
  "appTreeAttribution": "PID/creation identities and native clock correlation method",
  "frames": [
    {"action": "range", "trial": 1, "offsetFromInputMs": 10, "durationMs": 8}
  ]
}
```

Do not synthesize this file from rAF intervals. A 16.7 ms p95 is an optimization
target, separate from SC-002. Unavailable tracing remains visibly unmeasured.

## Soak interpretation and internal bounds

Full-run statistics use measured time after warmup. Soak reports crash count,
observed visible-update gaps, controls, regression slope over hours 18–24,
and median RSS during hours 2–3 versus 23–24. Short runs return null/pending for
these windows. Investigate >1 decimal MB/hour slope or >10 MB median growth;
these diagnostic thresholds do not establish absence of all leaks.

Internal counts are **test-proven**, not externally observed. Run:

```sh
cargo test --release --manifest-path src-tauri/Cargo.toml --target-dir src-tauri/target/invariants --features test-fixtures --test live_contract --test history_inspection --test inspection_contract --test memory_processes --test breakdown_contract
```

T021: <=1 in-flight and <=1 pending delivery, <=32 controls, <=256 KiB payload.
T046: <=3,601 history frames. T047: <=2 retained histories (one live, one frozen).
T054: <=8 memory and <=5 entries per ranking. This test-only feature is for the
invariant test binaries, never the executable under resource measurement.

`--feedback-trace native-feedback.json` enforces the actual SC-002 gate when
native event export is available. It requires source/platform/inputEvent/
presentationEvent/clockCorrelation/appTreeAttribution plus `feedback` entries
`{action, trial, inputMs, presentedMs}`. Each entry must identify the first
presented frame acknowledging that specific input, with correlated native
monotonic timestamps. Categories remain separate, duplicate trials and invalid
timestamps fail, and fewer than 20 trials stay pending. Any range/Pause/Resume
native p95 >200 ms fails the command. Settings timings are reported separately.
This file may be normalized after the raw trace run and supplied to a repeat
analysis using `aggregate_feedback` and the saved `controls.jsonl`; do not pair
trace events from one run with trials from a different run.

Invariant tests use a separate Cargo target directory because Cargo integration
tests can build/replace the application executable with test-only features.
Never profile an executable replaced by a fixture build, and do not rebuild or
patch an executable while measuring it. Rebuild with `npm run tauri build` after
any earlier shared-directory fixture run. The production executable hash and
normal-feature CLI build log identify the measured artifact.

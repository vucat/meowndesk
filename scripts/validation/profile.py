#!/usr/bin/env python3
"""Release app-tree benchmark. Defaults: 10m warmup, 30m measured, every 5s.

Use --binary ... --driver tauri-driver for native controls, --binary ... to launch
resources only, or --pid for an existing owned app. All short runs are provisional.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import statistics
import subprocess
import sys
import time

from common import (FrameTrace, Tree, WebDriver, aggregate_feedback, aggregate_frames, metadata,
                    owned_app_pid, p95, processes, slope_mb_hour, summary)

INVARIANTS = ['T021: live_contract (--features test-fixtures)',
              'T046: history_inspection', 'T047: inspection_contract (--features test-fixtures)',
              'T054: memory_processes; breakdown_contract']


def positive(value):
    number = float(value)
    if not 0 < number < float('inf'):
        raise argparse.ArgumentTypeError('must be finite and greater than zero')
    return number


def nonnegative(value):
    number = float(value)
    if not 0 <= number < float('inf'):
        raise argparse.ArgumentTypeError('must be finite and nonnegative')
    return number


def parser(soak=False):
    cli = argparse.ArgumentParser(description=__doc__)
    target = cli.add_mutually_exclusive_group(required=True)
    target.add_argument('--pid', type=int, help='Existing app root PID; never a browser broker or driver PID')
    target.add_argument('--binary', type=Path, help='Release executable; launched once and closed on completion')
    cli.add_argument('--driver', help='tauri-driver executable; enables real native UI actions')
    cli.add_argument('--duration', type=positive, default=86400 if soak else 1800, help='Measured seconds, excluding warmup')
    cli.add_argument('--warmup', type=nonnegative, default=600, help='Warmup seconds (still recorded)')
    cli.add_argument('--sample-interval', type=positive, default=60 if soak else 5)
    cli.add_argument('--poll-interval', type=positive, default=1, help='Visible-update observation period')
    cli.add_argument('--control-interval', type=positive, default=300 if soak else 1)
    cli.add_argument('--interval-ms', type=int, choices=[1000, 2000, 5000], default=1000,
                     help='Must match the app preference; the harness never edits preferences')
    cli.add_argument('--trials', type=int, default=20)
    cli.add_argument('--output', type=Path, default=Path('artifacts/validation/soak' if soak else 'artifacts/validation/profile'))
    cli.add_argument('--metadata', type=Path, help='Operator JSON: OS patch, driver/webview/GPU IDs, display/scaling and build provenance')
    cli.add_argument('--trace-config', type=Path, help='Linux Perfetto text config or Windows WPRP; no elevation requested')
    cli.add_argument('--frame-trace', type=Path, help='Normalized native frame trace JSON (see README)')
    cli.add_argument('--feedback-trace', type=Path, help='Native input/presentation event JSON for SC-002 gating')
    return cli


def control_summary(trials):
    result = {}
    for action in ['range', 'pause', 'resume', 'settings_open', 'settings_close']:
        values = [t['latencyMs'] for t in trials if t['action'] == action and 'latencyMs' in t]
        enough = len(values) >= 20
        result[action] = {'count': len(values), 'p95Ms': p95(values), 'maxMs': max(values) if values else None,
                          'proxyGate': 'failed' if enough and p95(values) > 200 else 'passed' if enough else 'pending 20 trials',
                          'sc002': 'pending native input-to-presentation validation' if action in ['range', 'pause', 'resume'] else 'reported separately',
                          'boundary': 'DOM acknowledgement followed by next rAF; provisional visible-feedback proxy'}
    return result


def soak_summary(samples, duration):
    # Never relabel an abbreviated run's final few seconds as the required six-hour window.
    full = duration >= 86400 and len(samples) > 1 and samples[-1]['elapsedSeconds'] >= 86400 - 120
    final = [s for s in samples if s['elapsedSeconds'] >= 18 * 3600]
    early = [s['rssBytes'] for s in samples if 2 * 3600 <= s['elapsedSeconds'] < 3 * 3600]
    late = [s['rssBytes'] for s in samples if 23 * 3600 <= s['elapsedSeconds'] <= 24 * 3600]
    slope = slope_mb_hour(final) if full else None
    growth = statistics.median(late) - statistics.median(early) if full and early and late else None
    return {'status': 'measured' if full and slope is not None and growth is not None else 'pending full 24-hour run',
            'finalSixHourSlopeMbPerHour': slope,
            'hours2to3MedianRssBytes': statistics.median(early) if full and early else None,
            'hours23to24MedianRssBytes': statistics.median(late) if full and late else None,
            'medianGrowthBytes': growth,
            'investigate': (slope > 1 or growth > 10_000_000) if slope is not None and growth is not None else None,
            'thresholds': 'Investigate >1 decimal MB/hour sustained slope or >10 MB median growth; not leak-proof gates'}


def run(args, soak=False):
    if args.driver and not args.binary:
        raise ValueError('--driver requires --binary so ownership can be established')
    if args.trials < 1:
        raise ValueError('--trials must be at least one')
    if args.binary and not args.binary.is_file():
        raise ValueError('Release binary does not exist')
    args.output.mkdir(parents=True, exist_ok=True)
    if (args.output / 'report.json').exists():
        raise ValueError('Output already has a report; use a new directory to preserve evidence')
    driver = None
    app = None
    trace = None
    samples, warmup_samples, trials, gaps, errors = [], [], [], [], []
    report = {'kind': 'soak' if soak else 'profile', 'metadata': metadata(args.binary),
              'requested': {'durationSeconds': args.duration, 'warmupSeconds': args.warmup,
                            'sampleIntervalSeconds': args.sample_interval, 'intervalMs': args.interval_ms},
              'invariants': {'classification': 'separate Rust invariant tests, not externally observed counts',
                             'evidenceStatus': 'available; inspect linked test results' if Path('artifacts/validation/phase7-invariants-release.log').is_file() else 'pending invariant test execution',
                             'suites': INVARIANTS, 'evidence': '../phase7-invariants-release.log'},
              'crashes': 0}
    if args.metadata:
        report['metadata']['operator'] = json.loads(args.metadata.read_text())
    started = time.monotonic()
    tool_cpu = time.process_time()
    exit_code = 0
    ready_seen = False
    try:
        if args.driver:
            driver = WebDriver(args.driver, args.binary, args.output)
            pid = owned_app_pid(driver, args.binary)
        elif args.binary:
            app = subprocess.Popen([str(args.binary.resolve())], stdout=(args.output / 'app.log').open('w'), stderr=subprocess.STDOUT)
            pid = app.pid
        else:
            pid = args.pid
        tree = Tree(pid, processes())
        trace = FrameTrace(args.trace_config, args.output)
        started = time.monotonic()
        deadline = started + args.warmup + args.duration
        next_sample = started
        next_control = started + args.warmup
        last_marker = None
        last_change = None
        pending = ['range', 'pause', 'resume', 'settings_open', 'settings_close'] * args.trials
        trial_numbers = dict.fromkeys(pending, 0)
        with (args.output / 'samples.jsonl').open('w') as series, (args.output / 'controls.jsonl').open('w') as controls:
            while time.monotonic() < deadline:
                now = time.monotonic()
                elapsed = now - started - args.warmup
                if now >= next_sample:
                    try:
                        row = tree.sample(processes(), time.monotonic())
                    except RuntimeError:
                        report['crashes'] += 1
                        raise
                    row.update(elapsedSeconds=elapsed, phase='warmup' if elapsed < 0 else 'measured')
                    (warmup_samples if elapsed < 0 else samples).append(row)
                    series.write(json.dumps(row) + '\n')
                    series.flush()
                    next_sample = time.monotonic() + args.sample_interval
                if driver:
                    status = driver.status()
                    ready_seen = ready_seen or status['ready']
                    if status['mode'] == 'live' and status['visible'] and status['ready']:
                        if status['marker'] != last_marker or last_change is None:
                            last_marker, last_change = status['marker'], now
                        elif now - last_change > 2 * args.interval_ms / 1000 + args.poll_interval:
                            gaps.append({'elapsedSeconds': elapsed, 'observedGapSeconds': now - last_change,
                                         'resolutionSeconds': args.poll_interval})
                            last_change = now
                    else:
                        last_change = None  # Pause/hide are intentional, not update failures.
                    if status['ready'] and elapsed >= 0 and now >= next_control and (soak or pending):
                        actions = ['range', 'pause', 'minimize_restore', 'resume'] if soak else [pending.pop(0)]
                        for action in actions:
                            trial_numbers[action] = trial_numbers.get(action, 0) + 1
                            row = {'action': action, 'trial': trial_numbers[action],
                                   'elapsedSeconds': time.monotonic() - started - args.warmup,
                                   'inputMonotonicMs': time.monotonic() * 1000}
                            try:
                                if action == 'minimize_restore':
                                    driver.minimize_restore()
                                    row['status'] = 'performed; window state requires native review'
                                else:
                                    row.update(driver.action(action))
                            except RuntimeError as error:
                                row['error'] = str(error)
                                errors.append(str(error))
                                # Keep the app live even when a driver's minimize operation is unavailable.
                            trials.append(row)
                            controls.write(json.dumps(row) + '\n')
                            controls.flush()
                        next_control = time.monotonic() + args.control_interval
                time.sleep(min(args.poll_interval, max(0, deadline - time.monotonic())))
    except (OSError, RuntimeError, ValueError, subprocess.SubprocessError, KeyboardInterrupt) as error:
        errors.append(str(error) or type(error).__name__)
        exit_code = 1
    finally:
        if trace:
            try:
                trace.close()
            except (OSError, RuntimeError, subprocess.SubprocessError) as error:
                errors.append(f'Trace cleanup failed: {error}')
                exit_code = 1
        if driver:
            try:
                driver.close()
            except (OSError, RuntimeError) as error:
                errors.append(f'Driver cleanup failed: {error}')
                exit_code = 1
        if app:
            app.terminate()
            try:
                app.wait(5)
            except subprocess.TimeoutExpired:
                app.kill()
                app.wait()
        actual = time.monotonic() - started
        report.update(actualSeconds=actual, resourceSummary=summary(samples), warmupSummary=summary(warmup_samples),
                      controls=control_summary(trials), visibleUpdateGaps=gaps if driver else None,
                      visibleUpdateStatus='observed by DOM footer, limited by poll period' if driver else 'pending; no native WebDriver',
                      errors=errors, traceCollection=trace.state if trace else {'status': 'unmeasured'})
        try:
            report['interactionFrames'] = aggregate_frames(args.frame_trace, trials)
            report['nativeFeedback'] = aggregate_feedback(args.feedback_trace, trials)
            if report['nativeFeedback']['status'] == 'failed':
                exit_code = 1
        except (ValueError, KeyError, OSError) as error:
            report['interactionFrames'] = {'status': 'invalid trace', 'error': str(error)}
            exit_code = 1
        report['measurementToolCpuSeconds'] = time.process_time() - tool_cpu
        report['measurementToolCpuNote'] = 'Python only; WebDriver/trace/CIM overhead excluded from app tree but not fully quantified'
        report['fullProtocol'] = 'pending' if errors or actual < args.warmup + args.duration or args.duration < (86400 if soak else 1800) or args.warmup < 600 or not driver or not ready_seen else 'duration completed; reference and native presentation review required'
        if soak:
            report['soak'] = soak_summary(samples, args.duration if not errors else 0)
        for action in ['range', 'pause', 'resume']:
            if report['controls'][action]['proxyGate'] == 'failed':
                exit_code = 1
        if driver and not ready_seen:
            errors.append('No displayed sample observed; verify embedded production assets and collection')
        if errors:
            exit_code = 1
        report['exitCode'] = exit_code
        (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps({'report': str(args.output / 'report.json'), 'exitCode': exit_code,
                          'samples': len(samples), 'fullProtocol': report['fullProtocol']}))
    return exit_code


if __name__ == '__main__':
    try:
        sys.exit(run(parser().parse_args()))
    except (ValueError, OSError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)

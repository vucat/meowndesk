"""External process accounting and optional W3C WebDriver; no application diagnostics API."""
from __future__ import annotations

import hashlib
import json
import math
import os
from pathlib import Path
import platform
import shutil
import socket
import statistics
import subprocess
import time
import urllib.error
import urllib.request
from dataclasses import dataclass


@dataclass(frozen=True)
class Process:
    pid: int
    parent: int
    identity: str
    cpu_seconds: float
    rss_bytes: int
    executable: str = ""


def processes() -> dict[int, Process]:
    if platform.system() == "Linux":
        result = {}
        ticks = os.sysconf("SC_CLK_TCK")
        page_size = os.sysconf("SC_PAGE_SIZE")
        for directory in Path('/proc').iterdir():
            if not directory.name.isdecimal():
                continue
            try:
                fields = (directory / 'stat').read_text().rsplit(')', 1)[1].split()
                try:
                    executable = os.readlink(directory / 'exe')
                except OSError:
                    executable = ''
                pid = int(directory.name)
                result[pid] = Process(pid, int(fields[1]), fields[19],
                                      (int(fields[11]) + int(fields[12])) / ticks,
                                      max(0, int(fields[21])) * page_size, executable)
            except (OSError, ValueError, IndexError):
                continue  # A process can exit between /proc enumeration and read.
        return result
    if platform.system() == "Windows":
        # No command lines or window titles are collected or emitted.
        script = """Get-CimInstance Win32_Process | Where-Object {$null -ne $_.CreationDate} | ForEach-Object {
          [pscustomobject]@{pid=$_.ProcessId;parent=$_.ParentProcessId;
          identity=$_.CreationDate.ToUniversalTime().ToString('o');
          cpu=([double]$_.KernelModeTime+[double]$_.UserModeTime)/10000000;
          rss=[string]$_.WorkingSetSize;exe=$_.ExecutablePath}
        } | ConvertTo-Json -Compress"""
        raw = subprocess.check_output(['powershell.exe', '-NoProfile', '-NonInteractive',
                                       '-Command', script], text=True, timeout=30)
        rows = json.loads(raw)
        if isinstance(rows, dict):
            rows = [rows]
        return {int(p['pid']): Process(int(p['pid']), int(p['parent']), p['identity'],
                                      float(p['cpu']), int(p['rss']), p['exe'] or '') for p in rows}
    raise RuntimeError('Only Linux /proc and Windows CIM accounting are supported')


class Tree:
    def __init__(self, pid: int, snapshot: dict[int, Process]):
        if pid not in snapshot:
            raise RuntimeError(f'App root PID {pid} is unavailable; cannot establish ownership')
        self.pid = pid
        self.root_identity = snapshot[pid].identity
        self.owned: dict[int, str] = {}
        self.previous: dict[tuple[int, str], float] = {}
        self.previous_time: float | None = None

    def sample(self, snapshot: dict[int, Process], now: float) -> dict:
        root = snapshot.get(self.pid)
        if root is None or root.identity != self.root_identity:
            raise RuntimeError('App root exited or its PID was reused')
        members = {pid for pid, identity in self.owned.items()
                   if pid in snapshot and snapshot[pid].identity == identity}
        members.add(self.pid)
        while True:
            children = {pid for pid, proc in snapshot.items() if proc.parent in members}
            added = children - members
            if not added:
                break
            members.update(added)
        self.owned = {pid: snapshot[pid].identity for pid in members}
        current = {(pid, snapshot[pid].identity): snapshot[pid].cpu_seconds for pid in members}
        wall = None if self.previous_time is None else now - self.previous_time
        # Newly observed helpers contribute their lifetime CPU since creation. Exited helpers'
        # last unsampled CPU is unavailable: record this accounting limit in every report.
        cpu_delta = sum(max(0, value - self.previous.get(key, value if wall is None else 0))
                        for key, value in current.items())
        cpu_pct = None if wall is None or wall <= 0 else 100 * cpu_delta / wall / (os.cpu_count() or 1)
        self.previous, self.previous_time = current, now
        pss = []
        if platform.system() == 'Linux':
            for pid in members:
                try:
                    lines = Path(f'/proc/{pid}/smaps_rollup').read_text().splitlines()
                    pss.append(int(next(line.split()[1] for line in lines if line.startswith('Pss:'))) * 1024)
                except (OSError, StopIteration, ValueError):
                    break
        return {'rssBytes': sum(snapshot[pid].rss_bytes for pid in members),
                'pssBytes': sum(pss) if len(pss) == len(members) else None,
                'cpuMachinePct': cpu_pct, 'cpuWindowSeconds': wall if wall is not None else 0, 'processCount': len(members),
                'members': sorted(members)}


def p95(values: list[float]) -> float | None:
    return sorted(values)[math.ceil(.95 * len(values)) - 1] if values else None


def summary(samples: list[dict]) -> dict:
    memory = [s['rssBytes'] for s in samples]
    cpu = [s['cpuMachinePct'] for s in samples if s['cpuMachinePct'] is not None]
    # Irregular sampling (especially CIM) uses elapsed-time weights for mean CPU.
    weighted = [(s['cpuMachinePct'], s.get('cpuWindowSeconds', 0)) for s in samples
                if s['cpuMachinePct'] is not None and s.get('cpuWindowSeconds', 0) > 0]
    return {'sampleCount': len(samples), 'meanRssBytes': statistics.mean(memory) if memory else None,
            'p95RssBytes': p95(memory), 'peakRssBytes': max(memory) if memory else None,
            'meanCpuMachinePct': (sum(v * w for v, w in weighted) / sum(w for _, w in weighted)) if weighted else None,
            'peakCpuMachinePct': max(cpu) if cpu else None,
            'targets': {'meanRssBytes': 60_000_000, 'meanCpuMachinePct': 1, 'classification': 'optimization goals; not release gates'}}


def slope_mb_hour(samples: list[dict]) -> float | None:
    if len(samples) < 2:
        return None
    xs = [s['elapsedSeconds'] / 3600 for s in samples]
    ys = [s['rssBytes'] / 1_000_000 for s in samples]
    x_mean, y_mean = statistics.mean(xs), statistics.mean(ys)
    denominator = sum((x - x_mean) ** 2 for x in xs)
    return sum((x - x_mean) * (y - y_mean) for x, y in zip(xs, ys)) / denominator if denominator else None


def metadata(binary: Path | None) -> dict:
    def digest(path: Path) -> str | None:
        return hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else None
    return {'startedAtUtc': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
            'platform': platform.platform(), 'logicalCpuCount': os.cpu_count(),
            'python': platform.python_version(), 'appVersion': '0.1.0',
            'binarySha256': digest(binary) if binary else None,
            'lockfiles': {str(p): digest(p) for p in [Path('package-lock.json'), Path('src-tauri/Cargo.lock')]},
            'accounting': 'sum RSS/working set of root and descendants; retain observed reparented helpers by PID+creation identity; exclude unrelated trees',
            'limits': ['RSS counts shared pages conservatively; PSS supplemental when readable',
                       'Exited helper CPU after its final observation cannot be recovered',
                       'Helpers never observed under app ancestry need independent ownership review',
                       'Native display scaling, drivers, GPU IDs and OS patch require operator metadata'],
            'referenceStatus': 'provisional until supplied native reference metadata is reviewed'}


class WebDriver:
    def __init__(self, driver: str, binary: Path, output: Path):
        self.process = None
        self.session = None
        self.log = (output / 'driver.log').open('w')
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        self.url = f'http://127.0.0.1:{port}'
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            native_port = sock.getsockname()[1]
        args = [driver, '--port', str(port), '--native-port', str(native_port)]
        native_driver = os.environ.get('NATIVE_WEBDRIVER')
        if native_driver:
            args += ['--native-driver', native_driver]
        self.process = subprocess.Popen(args, stdout=self.log, stderr=subprocess.STDOUT)
        try:
            deadline = time.monotonic() + 10
            while True:
                try:
                    self.request('GET', '/status')
                    break
                except (OSError, urllib.error.URLError):
                    if time.monotonic() >= deadline or self.process.poll() is not None:
                        raise RuntimeError('tauri-driver did not become ready; see driver.log')
                    time.sleep(.1)
            created = self.request('POST', '/session', {'capabilities': {'alwaysMatch': {
                'tauri:options': {'application': str(binary.resolve())}}}})
            self.session = created['sessionId']
            self.request('POST', self.path('/timeouts'), {'script': 8000})
            self.request('POST', self.path('/window/rect'), {'width': 1440, 'height': 1000})
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                try:
                    if self.status()['shellReady']:
                        break
                except RuntimeError as error:
                    if 'no such frame' not in str(error) and 'unload' not in str(error):
                        raise
                time.sleep(.05)
            else:
                raise RuntimeError('Bundled Overview did not load within 15s; check production build')
        except BaseException:
            self.close()
            raise

    def path(self, suffix: str) -> str:
        return f'/session/{self.session}{suffix}'

    def request(self, method: str, path: str, data: dict | None = None):
        request = urllib.request.Request(self.url + path, method=method,
                                        data=json.dumps(data).encode() if data is not None else None,
                                        headers={'Content-Type': 'application/json'})
        try:
            with urllib.request.urlopen(request, timeout=35) as response:
                value = json.load(response)['value']
        except urllib.error.HTTPError as error:
            raise RuntimeError(f'WebDriver {path}: {error.read().decode()[:500]}') from error
        if isinstance(value, dict) and value.get('error'):
            raise RuntimeError(str(value))
        return value

    def status(self) -> dict:
        return self.request('POST', self.path('/execute/sync'), {'script': """
          return {shellReady:!!document.querySelector('h1'), marker:document.querySelector('.overview-footer')?.textContent,
            mode:document.querySelector('.history-controls')?.dataset.mode,
            visible:document.visibilityState === 'visible',
            ready:!!document.querySelector('#history-pause:not(:disabled)') && (document.querySelector('.overview-footer')?.textContent||'').includes('Observed')};
        """, 'args': []})

    def action(self, action: str) -> dict:
        # Real WebDriver input; injected instrumentation observes DOM acknowledgement.
        selectors = {'range': '.range-control input:not(:checked) + span',
                     'pause': '#history-pause', 'resume': '#history-pause',
                     'settings_open': '#settings-trigger', 'settings_close': '.settings-cancel'}
        selector = selectors[action]
        self.request('POST', self.path('/execute/sync'), {'script': """
          const action=arguments[0], target=document.querySelector(arguments[1]);
          if(!target) throw new Error('Control unavailable');
          const rangeId=action==='range'?target.previousElementSibling.id:null;
          window.__meowndeskValidationTiming=null;
          target.addEventListener('click', ()=>{
            const start=performance.now();
            const acknowledged=()=>{
              if(action==='range') return document.getElementById(rangeId)?.checked;
              if(action==='pause'||action==='resume') return document.querySelector('#history-pause')?.textContent===(action==='pause'?'Resume':'Pause');
              return action==='settings_open'?!!document.querySelector('[role=dialog]'):!document.querySelector('[role=dialog]');
            };
            const check=()=>{
              if(acknowledged()) requestAnimationFrame(()=>{window.__meowndeskValidationTiming={latencyMs:performance.now()-start,
                boundary:'DOM click input to acknowledgement followed by next rAF; presentation proxy'};});
              else if(performance.now()-start>3000) window.__meowndeskValidationTiming={failure:'No visible acknowledgement within 3s'};
              else requestAnimationFrame(check);
            };requestAnimationFrame(check);
          }, {once:true});
          return true;
        """, 'args': [action, selector]})
        element = self.request('POST', self.path('/element'), {'using': 'css selector', 'value': selector})
        element_id = element['element-6066-11e4-a52e-4f735466cecf']
        self.request('POST', self.path(f'/element/{element_id}/click'), {})
        result = self.request('POST', self.path('/execute/async'), {'script': """
          const done=arguments[arguments.length-1], start=performance.now();
          const check=()=>{
            if(window.__meowndeskValidationTiming) done(window.__meowndeskValidationTiming);
            else if(performance.now()-start>5000) done({failure:'Input/acknowledgement observer timed out'});
            else setTimeout(check,10);
          };check();
        """, 'args': []})
        if 'failure' in result:
            raise RuntimeError(result['failure'])
        return result

    def minimize_restore(self):
        self.request('POST', self.path('/window/minimize'), {})
        time.sleep(.2)
        self.request('POST', self.path('/window/maximize'), {})

    def close(self):
        try:
            if self.session:
                self.request('DELETE', self.path(''))
        finally:
            if self.process:
                self.process.terminate()
                try:
                    self.process.wait(5)
                except subprocess.TimeoutExpired:
                    self.process.kill()
                    self.process.wait()
            self.log.close()


def owned_app_pid(driver: WebDriver, binary: Path) -> int:
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        snapshot = processes()
        owner = Tree(driver.process.pid, snapshot)
        members = owner.sample(snapshot, time.monotonic())['members']
        matches = [pid for pid in members if snapshot[pid].executable
                   and Path(snapshot[pid].executable).resolve() == binary.resolve()]
        if len(matches) == 1:
            return matches[0]
        time.sleep(.1)
    raise RuntimeError('Cannot uniquely attribute an app root under this WebDriver tree')


class FrameTrace:
    """Opt-in native trace capture. No suitable provider/config means unmeasured, never rAF frames."""
    def __init__(self, config: Path | None, output: Path):
        self.child = None
        self.output = output
        self.platform = platform.system()
        self.state = {'status': 'unmeasured', 'platform': self.platform,
                      'reason': 'No native trace configuration supplied; compositor event boundaries require platform instrumentation'}
        if config:
            if not config.is_file():
                raise RuntimeError('Native trace config does not exist')
            if self.platform == 'Linux' and shutil.which('perfetto'):
                self.child = subprocess.Popen(['perfetto', '--txt', '-c', str(config), '-o', str(output / 'native.pftrace')], stdout=subprocess.DEVNULL, stderr=(output / 'trace.log').open('w'))
                self.state = {'status': 'capturing; normalization/attribution pending', 'source': 'Linux Perfetto', 'raw': 'native.pftrace'}
            elif self.platform == 'Windows' and shutil.which('wpr'):
                subprocess.run(['wpr', '-start', str(config), '-filemode'], check=True, timeout=30, capture_output=True)
                self.state = {'status': 'capturing; normalization/attribution pending', 'source': 'Windows WPR/ETW', 'raw': 'native.etl'}
            else:
                self.state['reason'] = 'Native collector unavailable (Linux perfetto / Windows wpr)'

    def close(self):
        if self.child:
            self.child.terminate()
            self.child.wait(10)
            if self.child.returncode not in [0, -15]:
                self.state['status'] = 'failed trace collection'
        elif self.platform == 'Windows' and self.state.get('raw'):
            subprocess.run(['wpr', '-stop', str(self.output / 'native.etl')], check=True, timeout=60, capture_output=True)


def aggregate_frames(path: Path | None, trials: list[dict]) -> dict:
    if path is None:
        return {'status': 'unmeasured', 'reason': 'Native begin-to-render-submission trace not supplied; rAF is not frame-duration evidence'}
    data = json.loads(path.read_text())
    for key in ['source', 'platform', 'beginEvent', 'renderSubmissionEvent', 'appTreeAttribution']:
        if not data.get(key):
            raise ValueError(f'Normalized native frame trace requires {key}')
    if data['platform'] != platform.system():
        raise ValueError('Native frame trace platform does not match this measurement host')
    groups = {}
    for action in ['range', 'pause', 'resume']:
        relevant = [t for t in trials if t['action'] == action]
        durations = []
        covered = set()
        for frame in data['frames']:
            if frame['action'] != action:
                continue
            duration = float(frame['durationMs'])
            offset = float(frame['offsetFromInputMs'])
            if not math.isfinite(duration) or duration < 0 or not math.isfinite(offset):
                raise ValueError('Invalid native trace duration/offset')
            if 0 <= offset <= 500 and any(t['trial'] == frame['trial'] for t in relevant):
                durations.append(duration)
                covered.add(frame['trial'])
        groups[action] = {'count': len(durations), 'coveredTrials': len(covered), 'requiredTrials': 20,
                          'p95Ms': p95(durations), 'maxMs': max(durations) if durations else None,
                          'status': 'measured' if len(covered) >= 20 else 'pending incomplete trial coverage'}
    return {'status': 'measured' if all(g['status'] == 'measured' for g in groups.values()) else 'pending',
            'source': data['source'], 'platform': data['platform'], 'beginEvent': data['beginEvent'],
            'renderSubmissionEvent': data['renderSubmissionEvent'], 'appTreeAttribution': data['appTreeAttribution'],
            'windowMs': 500, 'actions': groups, 'targetMs': 16.7, 'classification': 'optimization target'}


def aggregate_feedback(path: Path | None, trials: list[dict]) -> dict:
    """Enforce SC-002 only with native input/presented-frame events on a correlated clock."""
    if path is None:
        return {'status': 'pending', 'reason': 'No native input-to-presented-frame trace supplied'}
    data = json.loads(path.read_text())
    for key in ['source', 'platform', 'inputEvent', 'presentationEvent', 'clockCorrelation', 'appTreeAttribution']:
        if not data.get(key):
            raise ValueError(f'Native feedback trace requires {key}')
    if data['platform'] != platform.system():
        raise ValueError('Native feedback trace platform mismatch')
    groups = {}
    for action in ['range', 'pause', 'resume', 'settings_open', 'settings_close']:
        available = {t['trial'] for t in trials if t['action'] == action}
        by_trial = {}
        for event in data['feedback']:
            if event['action'] != action or event['trial'] not in available:
                continue
            if event['trial'] in by_trial:
                raise ValueError('Duplicate feedback trial; require first acknowledging presented frame')
            duration = float(event['presentedMs']) - float(event['inputMs'])
            if not math.isfinite(duration) or duration < 0:
                raise ValueError('Invalid native input/presentation duration')
            by_trial[event['trial']] = duration
        values = list(by_trial.values())
        enough = len(values) >= 20
        gate = 'pending 20 trials'
        if enough:
            gate = ('passed' if p95(values) <= 200 else 'failed') if action in ['range', 'pause', 'resume'] else 'reported separately'
        groups[action] = {'count': len(values), 'p95Ms': p95(values), 'maxMs': max(values) if values else None, 'gate': gate}
    status = 'failed' if any(g['gate'] == 'failed' for g in groups.values()) else 'passed' if all(groups[a]['gate'] == 'passed' for a in ['range', 'pause', 'resume']) else 'pending'
    return {'status': status, 'source': data['source'], 'inputEvent': data['inputEvent'],
            'presentationEvent': data['presentationEvent'], 'clockCorrelation': data['clockCorrelation'],
            'actions': groups, 'limitMs': 200}

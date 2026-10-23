"""Statistics, ownership/PID reuse, trace windows and abbreviated-run honesty regressions."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from common import Process, Tree, aggregate_feedback, aggregate_frames, p95, slope_mb_hour
from profile import control_summary, soak_summary


class ValidationTests(unittest.TestCase):
    def test_nearest_rank_is_nineteenth_of_twenty_and_categories_do_not_pool(self):
        self.assertEqual(p95(list(range(1, 21))), 19)
        trials = [{'action': action, 'latencyMs': value} for action, value in
                  [('pause', 10)] * 20 + [('range', 201)] * 20 + [('resume', 1)] * 19]
        groups = control_summary(trials)
        self.assertEqual(groups['pause']['proxyGate'], 'passed')
        self.assertEqual(groups['range']['proxyGate'], 'failed')
        self.assertEqual(groups['resume']['proxyGate'], 'pending 20 trials')
        self.assertIn('pending native', groups['pause']['sc002'])

    @patch('common.os.cpu_count', return_value=4)
    def test_process_tree_excludes_other_browsers_retains_reparented_helpers_and_rejects_pid_reuse(self, _):
        snap = {1: Process(1, 0, 'app', 0, 100), 2: Process(2, 1, 'helper', 0, 200),
                3: Process(3, 0, 'unrelated-browser', 100, 9999)}
        tree = Tree(1, snap)
        self.assertEqual(tree.sample(snap, 0)['rssBytes'], 300)
        snap[2] = Process(2, 0, 'helper', 1, 200)
        result = tree.sample(snap, 1)
        self.assertEqual(result['processCount'], 2)
        self.assertEqual(result['cpuMachinePct'], 25)
        snap[2] = Process(2, 0, 'new-unrelated', 500, 9999)
        self.assertEqual(tree.sample(snap, 2)['rssBytes'], 100)
        snap[1] = Process(1, 0, 'reused-root', 0, 1)
        with self.assertRaises(RuntimeError):
            tree.sample(snap, 3)

    def test_soak_never_extrapolates_short_smoke_into_full_windows(self):
        rows = [{'elapsedSeconds': 0, 'rssBytes': 100}, {'elapsedSeconds': 10, 'rssBytes': 200}]
        self.assertIsNone(soak_summary(rows, 10)['finalSixHourSlopeMbPerHour'])
        self.assertAlmostEqual(slope_mb_hour([{'elapsedSeconds': 0, 'rssBytes': 0}, {'elapsedSeconds': 3600, 'rssBytes': 2_000_000}]), 2)

    def test_native_feedback_enforces_each_action_separately(self):
        import platform
        trials = [{'action': a, 'trial': i} for a in ['range', 'pause', 'resume'] for i in range(1, 21)]
        data = {'source': 'test native events', 'platform': platform.system(),
                'inputEvent': 'native_input', 'presentationEvent': 'compositor_present',
                'clockCorrelation': 'same monotonic clock', 'appTreeAttribution': 'owned test PID',
                'feedback': [{'action': t['action'], 'trial': t['trial'], 'inputMs': 0,
                              'presentedMs': 201 if t['action'] == 'pause' else 10} for t in trials]}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'feedback.json'
            path.write_text(json.dumps(data))
            result = aggregate_feedback(path, trials)
            self.assertEqual(result['status'], 'failed')
            self.assertEqual(result['actions']['range']['gate'], 'passed')
            self.assertEqual(result['actions']['pause']['gate'], 'failed')

    def test_native_frames_require_event_boundaries_and_filter_500ms_window(self):
        import platform
        data = {'source': 'test native trace', 'platform': platform.system(), 'beginEvent': 'frame_begin',
                'renderSubmissionEvent': 'render_submit', 'appTreeAttribution': 'explicit test PIDs',
                'frames': [{'action': 'range', 'trial': 1, 'offsetFromInputMs': 10, 'durationMs': 8},
                           {'action': 'range', 'trial': 1, 'offsetFromInputMs': 501, 'durationMs': 999}]}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'trace.json'
            path.write_text(json.dumps(data))
            result = aggregate_frames(path, [{'action': 'range', 'trial': 1}])
            self.assertEqual(result['actions']['range']['p95Ms'], 8)
            self.assertEqual(result['status'], 'pending')
            del data['beginEvent']
            path.write_text(json.dumps(data))
            with self.assertRaises(ValueError):
                aggregate_frames(path, [])


if __name__ == '__main__':
    unittest.main()

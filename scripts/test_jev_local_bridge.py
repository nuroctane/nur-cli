"""Adapter contract/device regressions; no models or optional packages required."""
import sys
import types
import unittest
from unittest.mock import patch

import jev_local_bridge as bridge


class AdapterTests(unittest.TestCase):
    def test_laya_preserves_choice_rubrics_and_score_criteria(self):
        backend = bridge.LayaBackend()
        seen = []

        class Agent:
            def predict(self, state, schema):
                seen.append(schema['decision'])
                return {'answers': {'decision': {'probabilities': {'0': .2, '1': .8}}}}

        backend._agent = Agent()
        backend.decide('choice', {'instructions': 'Route', 'criteria': {'a': 'Billing', 'b': 'Sales'}}, 'invoice')
        self.assertEqual(seen[-1]['criteria'], {'a': 'Billing', 'b': 'Sales'})
        self.assertNotIn('options', seen[-1])
        backend.decide('score', {'instructions': 'Rate', 'criteria': ['low', 'high']}, 'invoice')
        self.assertEqual(seen[-1]['criteria'], ['low', 'high'])

    def test_laya_rejects_old_macos_before_import(self):
        with patch.object(bridge.platform, 'system', return_value='Darwin'), patch.object(bridge.platform, 'machine', return_value='arm64'), patch.object(bridge.platform, 'mac_ver', return_value=('14.6', ('', '', ''), '')), patch.dict(sys.modules, {'laya_coreml': types.ModuleType('laya_coreml')}):
            ok, reason = bridge.LayaBackend().available()
        self.assertFalse(ok)
        self.assertIn('15', reason)

    def test_nimble_requires_native_bf16(self):
        cuda = types.SimpleNamespace(is_available=lambda: True, is_bf16_supported=lambda **kwargs: False)
        with patch('os.path.isfile', return_value=True), patch.dict(sys.modules, {'torch': types.SimpleNamespace(cuda=cuda)}):
            ok, reason = bridge.NimbleBackend().available()
        self.assertFalse(ok)
        self.assertIn('BF16', reason)

    def test_supported_device_probes_do_not_load_models(self):
        cuda = types.SimpleNamespace(is_available=lambda: True, is_bf16_supported=lambda **kwargs: True)
        with patch('os.path.isfile', return_value=True), patch.dict(sys.modules, {'torch': types.SimpleNamespace(cuda=cuda)}):
            self.assertTrue(bridge.NimbleBackend().available()[0])
        with patch.object(bridge.platform, 'system', return_value='Darwin'), patch.object(bridge.platform, 'machine', return_value='arm64'), patch.object(bridge.platform, 'mac_ver', return_value=('15.0', ('', '', ''), '')), patch.object(bridge.sys, 'version_info', (3, 12, 0)), patch.dict(sys.modules, {'laya_coreml': types.ModuleType('laya_coreml')}):
            self.assertTrue(bridge.LayaBackend().available()[0])

    def test_verdict_rejects_unavailable_requested_device(self):
        torch = types.SimpleNamespace(
            cuda=types.SimpleNamespace(is_available=lambda: False),
            backends=types.SimpleNamespace(mps=types.SimpleNamespace(is_available=lambda: False)),
        )
        with patch.dict(sys.modules, {'torch': torch}):
            ok, reason = bridge.VerdictBackend(device='cuda').available()
        self.assertFalse(ok)
        self.assertIn('CUDA', reason)

    def test_laya_enforces_bundle_option_limit(self):
        self.assertEqual(bridge.LayaBackend().max_options, 32)
        self.assertEqual(bridge.LayaBackend().max_levels, 32)

    def test_verdict_rejects_unrelated_core_module(self):
        with patch.dict(sys.modules, {'core': types.ModuleType('core')}):
            ok, _ = bridge.VerdictBackend().available()
        self.assertFalse(ok)


if __name__ == '__main__':
    unittest.main()

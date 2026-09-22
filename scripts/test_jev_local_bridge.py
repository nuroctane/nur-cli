"""Adapter contract/device regressions; no models or optional packages required."""
import sys
import types
import unittest
from unittest.mock import patch
from concurrent.futures import ThreadPoolExecutor

import jev_local_bridge as bridge


class AdapterTests(unittest.TestCase):
    def test_concurrent_requests_share_inference_for_each_real_adapter(self):
        body = {'state': 'x', 'questions': {'q': {'type': 'noul', 'instructions': 'x?'}}}
        for backend in [bridge.VerdictBackend(), bridge.NimbleBackend(), bridge.LayaBackend()]:
            with self.subTest(backend=backend.name), patch.object(backend, 'decide_batch', return_value=[{'true': .9}]) as infer:
                with ThreadPoolExecutor(max_workers=4) as pool:
                    results = list(pool.map(lambda _: bridge.handle_evaluate(backend, body), range(8)))
                self.assertEqual(infer.call_count, 1)
                self.assertTrue(all(result['answers']['q']['noul'] == .9 for result in results))
                self.assertEqual(backend.cache_hits, 7)

    def test_cache_reuses_exact_judgments_across_ids_but_not_state_or_schema(self):
        class Counting(bridge.MockBackend):
            calls = 0
            def decide_batch(self, items, state):
                self.calls += len(items)
                return super().decide_batch(items, state)

        backend = Counting()
        q = {'type': 'noul', 'instructions': 'duplicate charge?'}
        body = {'state': 'duplicate charge', 'questions': {'a': q, 'b': q}}
        first = bridge.handle_evaluate(backend, body)
        self.assertEqual(backend.calls, 1)
        second = bridge.handle_evaluate(backend, {'state': body['state'], 'questions': {'c': q}})
        self.assertEqual(first['answers']['a'], second['answers']['c'])
        self.assertEqual(backend.calls, 1)
        bridge.handle_evaluate(backend, {'state': 'changed', 'questions': {'a': q}})
        bridge.handle_evaluate(backend, {'state': body['state'], 'questions': {'a': dict(q, instructions='refund?')}})
        self.assertEqual(backend.calls, 3)
        self.assertEqual(backend.cache_hits, 2)

    def test_cache_is_bounded_expires_and_can_be_disabled(self):
        backend = bridge.MockBackend()
        backend.cache_size = 1
        q = {'type': 'noul', 'instructions': 'charge?'}
        def ask(state):
            return bridge.handle_evaluate(backend, {'state': state, 'questions': {'q': q}})
        with patch.object(bridge.time, 'monotonic', return_value=10):
            ask('one')
            ask('two')
            self.assertEqual(len(backend._decision_cache), 1)
            ask('one')
            self.assertEqual(backend.cache_hits, 0)
            ask('one')
            self.assertEqual(backend.cache_hits, 1)
        with patch.object(bridge.time, 'monotonic', return_value=71):
            ask('one')
            self.assertEqual(backend.cache_hits, 1)
        backend.cache_size = 0
        ask('one')
        self.assertEqual(backend.cache_hits, 1)

    def test_cache_never_saves_malformed_answers_or_backend_failures(self):
        class Broken(bridge.MockBackend):
            calls = 0
            def decide_batch(self, items, state):
                self.calls += 1
                if self.calls == 1:
                    raise RuntimeError('temporary failure')
                if self.calls == 2:
                    return [{} for _ in items]
                return [{'true': .9} for _ in items]
        backend = Broken()
        body = {'state': 'charge', 'questions': {'q': {'type': 'noul', 'instructions': 'charge?'}}}
        self.assertFalse(bridge.handle_evaluate(backend, body)['answers'])
        self.assertFalse(bridge.handle_evaluate(backend, body)['answers'])
        self.assertTrue(bridge.handle_evaluate(backend, body)['answers'])
        self.assertEqual(backend.calls, 3)

    def test_token_estimate_matches_rust_runs_and_ignores_whitespace(self):
        for text, expected in [('abc def', 2), ('abcdefg', 2), ('1 2', 1), ('123', 2),
                               (' \n\t', 0), ('{}', 2), ('漢字', 2)]:
            self.assertEqual(bridge.estimate_tokens(text), expected, text)

    def test_nonfinite_noul_never_becomes_a_confident_yes(self):
        for value in [float('nan'), float('inf'), float('-inf')]:
            with self.assertRaises(bridge.ContractError):
                bridge.answer_for('noul', {}, {'true': value})

    def test_cache_keeps_valid_answers_when_another_distribution_is_malformed(self):
        class Partial(bridge.MockBackend):
            def decide_batch(self, items, state):
                return [{'true': .9}, None]
        result = bridge.handle_evaluate(Partial(), {'state': 'x', 'questions': {
            'a': {'type': 'noul', 'instructions': 'one'},
            'b': {'type': 'noul', 'instructions': 'two'},
        }})
        self.assertIn('a', result['answers'])
        self.assertNotIn('b', result['answers'])
        self.assertTrue(result['errors'])

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

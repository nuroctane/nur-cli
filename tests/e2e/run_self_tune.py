"""Black-box train/held-out acceptance for bench self-tuning."""
import argparse
import json
import os
import subprocess
import threading
import time

from fake_provider import LoopbackHTTPServer, State, make_handler
from run_e2e import OUT, find_binary, isolated_env, spawn_process, stop_tree


def run(binary):
    work = OUT / f'self-tune-{time.time_ns()}'
    home, workspace = work / 'home', work / 'workspace'
    workspace.mkdir(parents=True)
    log = work / 'requests.jsonl'
    write = {'tool_calls': [{'name': 'write_file', 'arguments': {'path': 'good.txt', 'contents': 'done'}}]}
    state = State([
        {'text': 'baseline train failed'}, write, {'text': 'baseline holdout passed'},
        {'text': 'Candidate one'}, write, {'text': 'train improved'}, {'text': 'holdout regressed'},
        {'text': 'Candidate two'}, write, {'text': 'train improved'}, write, {'text': 'holdout preserved'},
        {'text': 'Candidate two'},
        {'text': 'Project tuning loaded.'},
        {'text': 'Subdirectory tuning loaded.'},
        {'text': 'Worktree tuning loaded.'},
        {'text': 'Other model unchanged.'},
        {'text': 'Spoofed project unchanged.'},
        {'text': 'Other project unchanged.'},
        write, {'text': 'Existing baseline train.'}, write, {'text': 'Existing baseline held-out.'},
        {'text': 'Candidate regression'}, {'text': 'Candidate failed.'},
        {'text': 'Previously accepted tuning remains.'},
        {'text': 'Tuning disabled.'},
    ], log)
    server = LoopbackHTTPServer(('127.0.0.1', 0), make_handler(state))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    env = isolated_env(home, server.server_port)
    with (home / '.nur/config.toml').open('a', encoding='utf-8') as config:
        config.write('\n[sandbox]\nmode = "off"\n')
    bench = home / '.nur/bench'
    bench.mkdir(parents=True, exist_ok=True)
    check = 'python check.py'
    for name, prompt in [('train', 'TRAIN_FIXTURE'), ('holdout', 'PRIVATE_HELDOUT_FIXTURE')]:
        (bench / f'{name}.json').write_text(json.dumps({'name': name, 'prompt': prompt, 'check': check}), encoding='utf-8')
    def git(*args):
        subprocess.run(['git', *args], cwd=workspace, env=env, check=True, capture_output=True)
    git('init')
    (workspace / 'fixture.txt').write_text('fixture', encoding='utf-8')
    (workspace / 'check.py').write_text("import pathlib,sys\nsys.exit(not pathlib.Path('good.txt').exists())\n", encoding='utf-8')
    git('add', 'fixture.txt', 'check.py')
    git('-c', 'user.name=E2E', '-c', 'user.email=e2e@example.invalid', 'commit', '-m', 'fixture')
    try:
        overlap = subprocess.run([str(binary), 'bench', 'self-tune', '--train', 'train', '--held-out', 'train'], cwd=workspace, env=env, capture_output=True, text=True, timeout=30)
        assert overlap.returncode != 0 and 'disjoint' in overlap.stderr, overlap.stdout + overlap.stderr
        assert state.count == 0, 'overlapping sets made provider calls'
        if os.name == 'nt':
            alias = subprocess.run([str(binary), 'bench', 'self-tune', '--train', 'train', '--held-out', 'TRAIN'], cwd=workspace, env=env, capture_output=True, text=True, timeout=30)
            assert alias.returncode != 0 and 'disjoint' in alias.stderr, alias.stdout + alias.stderr
            assert state.count == 0, 'case-insensitive aliases made provider calls'
        proc = spawn_process([str(binary), 'bench', 'self-tune', '--train', 'train', '--held-out', 'holdout', '--rounds', '3'], cwd=workspace, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, start_new_session=os.name != 'nt')
        try:
            stdout, stderr = proc.communicate(timeout=180)
            assert proc.returncode == 0, stdout + stderr
        finally:
            stop_tree(proc)
        records = [json.loads(line) for line in (bench / 'self-tune.jsonl').read_text(encoding='utf-8').splitlines()]
        assert [row['decision'] for row in records] == ['rejected_holdout', 'accepting', 'accepted', 'rejected_invalid'], records
        assert len({row['run_id'] for row in records}) == 1 and all(row['train_tasks'] == ['train'] and row['held_out_tasks'] == ['holdout'] and row['baseline_held_out_pass_rate'] == 1 for row in records) and [row['baseline_train_pass_rate'] for row in records] == [0, 0, 0, 1] and all(row['provider'] == 'vllm' and row['model'] == 'e2e-model' for row in records), records
        assert [path.read_text(encoding='utf-8').strip() for path in (bench / 'tuned').glob('*.txt')] == ['Candidate two']
        requests = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
        proposals = [r for r in requests if 'tuning the standing instruction' in json.dumps(r)]
        assert len(proposals) == 3 and all('PRIVATE_HELDOUT_FIXTURE' not in json.dumps(r) for r in proposals)
        expected = '[Tuned project instruction]\nCandidate two\n\n[User request]\nTRAIN_FIXTURE'
        assert any(json.dumps(expected)[1:-1] in json.dumps(r) for r in requests), 'candidate was not evaluated in its activation format'
        result = subprocess.run([str(binary), 'run', 'A normal project task'], cwd=workspace, env=env, capture_output=True, text=True, timeout=45)
        assert result.returncode == 0, result.stdout + result.stderr
        requests = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
        assert 'Candidate two' in json.dumps(requests[-1]), 'accepted instruction was not applied'
        subdir = workspace / 'subdirectory'
        subdir.mkdir()
        result = subprocess.run([str(binary), 'run', 'A subdirectory task'], cwd=subdir, env=env, capture_output=True, text=True, timeout=45)
        assert result.returncode == 0, result.stdout + result.stderr
        requests = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
        assert 'Candidate two' in json.dumps(requests[-1]), 'tuning did not follow the project root'
        linked = work / 'linked-worktree'
        help_result = subprocess.run(['git', 'worktree', 'add', '-h'], capture_output=True, text=True)
        help_text = help_result.stdout + help_result.stderr
        relative = ['--relative-paths'] if 'relative-paths' in help_text else []
        git('worktree', 'add', '--detach', *relative, str(linked))
        try:
            result = subprocess.run([str(binary), 'run', 'A linked worktree task'], cwd=linked, env=env, capture_output=True, text=True, timeout=45)
            assert result.returncode == 0, result.stdout + result.stderr
            requests = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
            assert 'Candidate two' in json.dumps(requests[-1]), 'tuning did not follow the linked worktree'
        finally:
            git('worktree', 'remove', str(linked))
        result = subprocess.run([str(binary), '--model', 'other-model', 'run', 'A different model task'], cwd=workspace, env=env, capture_output=True, text=True, timeout=45)
        assert result.returncode == 0, result.stdout + result.stderr
        requests = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
        assert 'Candidate two' not in json.dumps(requests[-1]), 'tuning leaked to an unevaluated model'
        spoof = work / 'spoof-project'
        spoof.mkdir()
        (spoof / '.git').write_text(f"gitdir: {(workspace / '.git').as_posix()}\n", encoding='utf-8')
        result = subprocess.run([str(binary), 'run', 'A spoofed project task'], cwd=spoof, env=env, capture_output=True, text=True, timeout=45)
        assert result.returncode == 0, result.stdout + result.stderr
        requests = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
        assert 'Candidate two' not in json.dumps(requests[-1]), 'a forged Git pointer imported another project tuning'
        other = work / 'other-project'
        other.mkdir()
        result = subprocess.run([str(binary), 'run', 'A separate project task'], cwd=other, env=env, capture_output=True, text=True, timeout=45)
        assert result.returncode == 0, result.stdout + result.stderr
        requests = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
        assert 'Candidate two' not in json.dumps(requests[-1]), 'tuned instruction leaked to another project'
        second = subprocess.run([str(binary), 'bench', 'self-tune', '--train', 'train', '--held-out', 'holdout'], cwd=workspace, env=env, capture_output=True, text=True, timeout=180)
        assert second.returncode == 0, second.stdout + second.stderr
        records = [json.loads(line) for line in (bench / 'self-tune.jsonl').read_text(encoding='utf-8').splitlines()]
        assert records[-1]['decision'] == 'rejected_train' and records[-1]['baseline_train_pass_rate'] == 1, records[-1]
        assert [path.read_text(encoding='utf-8').strip() for path in (bench / 'tuned').glob('*.txt')] == ['Candidate two']
        result = subprocess.run([str(binary), 'run', 'Check retained tuning'], cwd=workspace, env=env, capture_output=True, text=True, timeout=45)
        assert result.returncode == 0, result.stdout + result.stderr
        requests = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
        assert 'Candidate two' in json.dumps(requests[-1]), 'a rejected candidate replaced accepted tuning'
        tuned = list((bench / 'tuned').glob('*.txt'))
        assert len(tuned) == 1
        tuned[0].unlink()
        result = subprocess.run([str(binary), 'run', 'Check disabled tuning'], cwd=workspace, env=env, capture_output=True, text=True, timeout=45)
        assert result.returncode == 0, result.stdout + result.stderr
        requests = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
        assert 'Candidate two' not in json.dumps(requests[-1]), 'deleted tuning remained active'
        print('PASS bench-self-tune')
    finally:
        server.shutdown()
        server.server_close()


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--bin')
    run(find_binary(parser.parse_args().bin))

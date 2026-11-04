"""Real-terminal regressions for the v0.61 secrets and continuation UI.

Uses only an isolated home and scripted loopback provider. No clipboard access.
Run with --bin PATH [secret-cancel|secret-click|secret-repeat|resume-controls].
"""
import argparse
import json
import re
import threading
import time

from run_e2e import find_binary, isolated_env
from run_startup import Terminal, LoopbackHTTPServer, OUT
from fake_provider import State, make_handler


TOKEN = 'ghp_aB3dE5fG7hJ9kL1mN3pQ5rS7tU9vW1xY3zA5'


def run(binary, scenario):
    work = OUT / f'{scenario}-{time.time_ns()}'
    work.mkdir(parents=True)
    home, workspace = work/'home', work/'workspace'
    workspace.mkdir()
    log = work/'requests.jsonl'
    script = [{'text': 'SECRET_FLOW_COMPLETED'}]
    if scenario == 'admission-new-session':
        script = [
            {'tool_calls': [{'name': 'agent', 'arguments': {'description': 'new session child', 'prompt': 'Review briefly.', 'async': True}}]},
            {'match_child': True, 'text': 'Child complete.'},
            {'match_child': False, 'tool_calls': [{'name': 'admission', 'arguments': {'action': 'list'}}]},
            {'match_child': False, 'text': 'ADMISSION_CHECKED'},
            {'match_child': False, 'tool_calls': [{'name': 'admission', 'arguments': {'action': 'list'}}]},
            {'match_child': False, 'text': 'NEW_SESSION_CHECKED'},
        ]
    if scenario == 'worker-background-cancel':
        script = [
            {'tool_calls': [{'name': 'agent', 'arguments': {'description': 'slow child', 'prompt': 'Run the fixture.', 'subagent_type': 'general'}}]},
            {'tool_calls': [{'name': 'bash', 'arguments': {'command': 'python -c "import time; time.sleep(9); open(\'late-marker.txt\',\'w\').write(\'late\')"'}}]},
            {'text': 'waiting\n', 'stream_delay': 8},
        ]
    if scenario == 'worker-background-session':
        script = [
            {'tool_calls': [{'name': 'agent', 'arguments': {'description': 'slow child', 'prompt': 'Run the fixture.', 'subagent_type': 'general'}}]},
            {'tool_calls': [{'name': 'bash', 'arguments': {'command': 'python -c "import time; time.sleep(8); print(\'OLD_SESSION_WORKER_OUTPUT\')"'}}]},
            {'text': 'Child handed off the command.'},
            {'text': 'FIRST_SESSION_DONE'},
            {'text': 'SECOND_SESSION_DONE'},
        ]
    if scenario in ('swarm-question', 'swarm-question-session'):
        script = [
            {'match_child': False, 'tool_calls': [{'name': 'agent', 'arguments': {'description': 'async question', 'prompt': 'Ask the user which track.', 'subagent_type': 'general', 'async': True}}]},
            {'match_child': True, 'tool_calls': [{'name': 'bash', 'arguments': {'command': 'python -c "import time;time.sleep(3)"'}}]},
            {'match_child': False, 'text': 'PARENT_FINISHED'},
            {'match_child': True, 'tool_calls': [{'name': 'question', 'arguments': {'question': 'CHILD_GATE_QUESTION', 'options': [{'label': 'Stable'}, {'label': 'Preview'}]}}]},
            {'match_child': True, 'text': 'Child received the answer.'},
            {'match_child': False, 'text': 'NEW_SESSION_OK'},
        ]
    if scenario == 'swarm-question-queue':
        script = [
            {'match_child': False, 'tool_calls': [{'name': 'agent', 'arguments': {'description': 'async question', 'prompt': 'Ask the child question.', 'subagent_type': 'general', 'async': True}}]},
            {'match_child': True, 'tool_calls': [{'name': 'bash', 'arguments': {'command': 'python -c "import time;time.sleep(3)"'}}]},
            {'match_child': False, 'tool_calls': [{'name': 'question', 'arguments': {'question': 'ROOT_FIRST_QUESTION', 'options': [{'label': 'One'}, {'label': 'Two'}]}}]},
            {'match_child': True, 'tool_calls': [{'name': 'question', 'arguments': {'question': 'CHILD_QUEUED_QUESTION', 'options': [{'label': 'Stable'}, {'label': 'Preview'}]}}]},
            {'match_child': False, 'text': 'ROOT_ANSWERED'},
            {'match_child': True, 'text': 'CHILD_ANSWERED'},
        ]
    if scenario in ('swarm-approval', 'swarm-approval-session'):
        (workspace/'.nur').mkdir()
        (workspace/'.nur/permissions.toml').write_text('ask = ["write_file"]\n', encoding='utf-8')
        script = [
            {'match_child': False, 'tool_calls': [{'name': 'agent', 'arguments': {'description': 'async writer', 'prompt': 'Run the fixture and write a file.', 'subagent_type': 'general', 'async': True}}]},
            {'match_child': True, 'tool_calls': [{'name': 'bash', 'arguments': {'command': 'python -c "import time;time.sleep(3)"'}}]},
            {'match_child': False, 'text': 'PARENT_FINISHED'},
            {'match_child': True, 'tool_calls': [{'name': 'write_file', 'arguments': {'path': 'child.txt', 'content': 'not authorized'}}]},
            {'match_child': True, 'text': 'Child acknowledged denial.'},
            {'match_child': False, 'text': 'NEW_SESSION_OK'},
        ]
    if scenario.startswith('secret-request'):
        script.insert(0, {'tool_calls': [{'name': 'secret', 'arguments': {
            'action': 'request', 'name': 'SERVICE_TOKEN', 'purpose': 'Access the test service'}}]})
    if scenario == 'resume-controls':
        script = [{'status': 429, 'body': {'error': {
            'message': 'Usage limit reached', 'resets_in_seconds': 120}}}] * 4 + script
    state = State(script, log)
    server = LoopbackHTTPServer(('127.0.0.1', 0), make_handler(state))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    env = isolated_env(home, server.server_port)
    env.pop('NO_COLOR', None)
    if scenario == 'secret-reduced-motion':
        env['NUR_REDUCED_MOTION'] = '1'
    (home/'.nur/config.toml').write_text(
        f'provider = "vllm"\nmodel = "e2e-model"\nbase_url = "http://127.0.0.1:{server.server_port}/v1"\n'
        'theme = "gold"\necosystem_auto_ensure = false\nauto_update = false\nnative_memory = false\n'
        '[typesafe]\nenabled = false\n[sandbox]\nmode = "off"\n' +
        ('[notifications]\nchime = false\n' if scenario == 'chime-disabled' else ''), encoding='utf-8')
    term = None

    def command(text):
        if scenario == 'secret-small':
            # Narrow palettes overlap the draft cursor, so wait between real
            # keystrokes instead of looking for the whole draft on one row.
            for char in text:
                term.write(char)
                term.pump(.12)
        else:
            term.type_text(text)
        term.write('\r')

    def paste(text):
        term.write('\x1b[200~' + text + '\x1b[201~')
        term.pump(.2)

    def click(label):
        term.until(label)
        rows = term.screen.display
        y = next(y for y, row in enumerate(rows) if label in row)
        x = rows[y].index(label)
        term.write(f'\x1b[<0;{x+1};{y+1}M\x1b[<0;{x+1};{y+1}m')
        term.pump(.2)

    try:
        size = {'columns': 32, 'rows': 10} if scenario == 'secret-small' else {}
        args = ['--yes', '/secrets add X'] if scenario == 'secret-small' else ['--yes']
        term = Terminal(binary, workspace, env, args=args, **size)
        if scenario != 'secret-small':
            term.until('F6 inspect', timeout=10)
        if scenario in ('swarm-approval', 'swarm-approval-session'):
            command('Start the asynchronous writer')
            term.until('PARENT_FINISHED', timeout=30)
            if scenario == 'swarm-approval-session':
                old_session = next(p.stem for p in (home/'.nur/sessions').glob('*.json') if not p.name.endswith('.status.json'))
                command('/new')
                term.until('new session')
                term.pump(5)
                assert 'write_file' not in '\n'.join(term.screen.display), 'old session approval leaked into the new session'
                command('Continue the new session')
                term.until('NEW_SESSION_OK', timeout=30)
                command('/resume ' + old_session)
            term.until('write_file', timeout=30)
            term.until('subagent approve', timeout=10)
            (work/'approval-screen.txt').write_text('\n'.join(term.screen.display), encoding='utf-8')
            term.write('n')
            term.until('child denial delivered', timeout=30, found=lambda: log.exists() and 'user denied this tool call' in log.read_text(encoding='utf-8'))
            assert not (workspace/'child.txt').exists(), 'denied child write occurred'
        elif scenario == 'swarm-question-queue':
            command('Start a child and ask me a question')
            term.until('ROOT_FIRST_QUESTION', timeout=30)
            term.until('child request started', timeout=30, found=lambda: state.count >= 4)
            term.pump(1)
            assert 'ROOT_FIRST_QUESTION' in '\n'.join(term.screen.display), 'child question replaced the root dialog'
            term.write('1')
            term.until('CHILD_QUEUED_QUESTION', timeout=30)
            term.write('2')
            term.until('both answers delivered', timeout=30, found=lambda: log.exists() and 'user selected: One' in log.read_text(encoding='utf-8') and 'user selected: Preview' in log.read_text(encoding='utf-8'))
        elif scenario in ('swarm-question', 'swarm-question-session'):
            command('Start an asynchronous child')
            term.until('PARENT_FINISHED', timeout=30)
            if scenario == 'swarm-question-session':
                old_session = next(p.stem for p in (home/'.nur/sessions').glob('*.json') if not p.name.endswith('.status.json'))
                command('/new')
                term.until('new session')
                term.pump(5)
                assert 'CHILD_GATE_QUESTION' not in '\n'.join(term.screen.display), 'old session question leaked into the new session'
                command('/swarm detail')
                term.pump(1)
                assert 'Ask the user which track' not in '\n'.join(term.screen.display), 'old session swarm details leaked into the new session'
                command('Continue the new session')
                term.until('NEW_SESSION_OK', timeout=30)
                command('/resume ' + old_session)
                term.until('CHILD_GATE_QUESTION', timeout=30)
                term.write('2')
                term.until('child answer delivered after returning', timeout=30, found=lambda: log.exists() and 'user selected: Preview' in log.read_text(encoding='utf-8'))
            else:
                term.until('CHILD_GATE_QUESTION', timeout=30)
                (work/'question-screen.txt').write_text('\n'.join(term.screen.display), encoding='utf-8')
                term.write('2')
                term.until('child answer delivered', timeout=30, found=lambda: log.exists() and 'user selected: Preview' in log.read_text(encoding='utf-8'))
        elif scenario == 'worker-background-session':
            command('Have a child run the slow fixture')
            term.until('FIRST_SESSION_DONE', timeout=30)
            command('/new')
            term.until('new session')
            term.pump(5)
            results = list((home/'.nur/bg-jobs').glob('*.txt'))
            assert any('OLD_SESSION_WORKER_OUTPUT' in p.read_text(encoding='utf-8') for p in results), 'worker did not complete'
            command('Continue in this new session')
            term.until('SECOND_SESSION_DONE', timeout=30)
            requests = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
            assert 'OLD_SESSION_WORKER_OUTPUT' not in json.dumps(requests[-1]), 'worker completion leaked into a new session'
        elif scenario == 'worker-background-cancel':
            command('Have a child run the slow fixture')
            term.until('child resumed after handoff', timeout=30, found=lambda: state.count >= 3)
            term.write('\x1b')
            term.pump(6)
            assert not (workspace/'late-marker.txt').exists(), 'cancelled child background command kept running'
        elif scenario == 'admission-new-session':
            command('Start a child and list its admission')
            term.until('ADMISSION_CHECKED', timeout=30)
            assert '1 admission(s)' in log.read_text(encoding='utf-8'), 'admission was not visible in the new session'
            command('/new')
            term.until('new session')
            command('List admissions for this session')
            term.until('NEW_SESSION_CHECKED', timeout=30)
            assert 'no admitted subagents in this session' in log.read_text(encoding='utf-8'), 'admission leaked into a new session'
        elif scenario == 'loop-session':
            command('/loop 2s Check the fixture')
            command('/new')
            term.until('loop stopped after switching sessions', timeout=10)
            term.pump(2.5)
            assert state.count == 0, 'loop leaked into a new session'
        elif scenario == 'loop-repeat':
            command('/loop 1s Check the fixture')
            term.until('two loop turns', timeout=30, found=lambda: state.count >= 2)
            command('/loop off')
            term.until('loop stopped', timeout=10)
            term.pump(1)
            stopped_count = state.count
            term.pump(2.5)
            assert state.count == stopped_count, 'loop continued after it was stopped'
        elif scenario in ('chime-enabled', 'chime-disabled'):
            # OSC title updates also terminate with BEL; count only bells
            # outside those control sequences.
            bell_count = lambda: re.sub(r'\x1b\][^\x07]*\x07', '', term.output).count('\x07')
            before = bell_count()
            command('Finish the task')
            term.until('SECRET_FLOW_COMPLETED', timeout=30)
            term.pump(1)
            observed = bell_count() - before
            assert observed == (1 if scenario == 'chime-enabled' else 0), f'expected bell setting, got {observed}'
        elif scenario == 'secret-small':
            term.until('New secret')
            term.until('value')
            paste('abcdefghi')
            click('save')
            term.until('saved $X')
        elif scenario in ('secret-animation', 'secret-reduced-motion'):
            command('/secrets add X')
            term.until('New secret')
            def border():
                return tuple((x, y, cell.fg) for y, row in term.screen.buffer.items()
                             for x, cell in row.items() if cell.data in '═║')
            samples = []
            for _ in range(6):
                term.pump(.15)
                samples.append(border())
            changed = any(sample != samples[0] for sample in samples[1:])
            assert changed == (scenario == 'secret-animation'), 'modal motion preference ignored'
            term.write('\x1b')
            term.until('closed modal', found=lambda: 'New secret' not in '\n'.join(term.screen.display))
        elif scenario == 'secret-cancel':
            command('/secrets add TEST_TOKEN')
            term.until('New secret')
            paste('masked_value_123')
            assert 'masked_value_123' not in term.output
            # Ctrl+C cancels the modal without quitting or modifying the draft.
            term.write('\x03')
            term.until('closed secret modal', found=lambda: 'New secret' not in '\n'.join(term.screen.display))
            assert not (home/'.nur/secrets.json').exists()
            command('/secrets add TEST_TOKEN')
            term.until('New secret')
            click('✕')
            term.until('closed secret modal', found=lambda: 'New secret' not in '\n'.join(term.screen.display))
        elif scenario == 'secret-click':
            command('/secrets add TEST_TOKEN')
            term.until('New secret')
            paste('masked_value_123')
            # A visible save action must also work with the mouse.
            click('save')
            term.until('saved $TEST_TOKEN')
            store = json.loads((home/'.nur/secrets.json').read_text())
            assert store['secrets']['TEST_TOKEN']['value'] == 'masked_value_123'
        elif scenario == 'secret-validation':
            command('/secrets add')
            term.until('New secret')
            paste('my-token')
            term.write('\t')
            term.pump(.1)
            paste('bad')
            click('save')
            term.until('a secret needs at least 6 characters')
            # Validation focuses the field needing correction. Ctrl+U only
            # clears that field; the normalized name must survive.
            term.write('\x15')
            paste('replacement_value_123')
            term.write('\r')
            term.until('saved $MY_TOKEN')
            store = json.loads((home/'.nur/secrets.json').read_text())
            assert store['secrets']['MY_TOKEN']['value'] == 'replacement_value_123'
            assert 'replacement_value_123' not in term.output
        elif scenario == 'secret-repeat':
            command(f'Use {TOKEN} and {TOKEN} for the migration')
            term.until('in your message')
            # Saving one credential replaces every occurrence in the held message.
            term.write('\r')
            term.until('SECRET_FLOW_COMPLETED', timeout=30)
            requests = log.read_text()
            assert TOKEN not in requests
            assert '$GITHUB_TOKEN and $GITHUB_TOKEN' in requests
            assert TOKEN not in (home/'.nur/history.jsonl').read_text()
        elif scenario.startswith('secret-request'):
            command('Access the service')
            term.until('The agent needs a secret', timeout=30)
            term.until('Access the test service')
            if scenario == 'secret-request-cancel':
                term.write('\x1b')
            else:
                paste('service_token_12345')
                term.write('\r')
            term.until('SECRET_FLOW_COMPLETED', timeout=30)
            requests = log.read_text()
            assert 'service_token_12345' not in requests
            if scenario == 'secret-request-cancel':
                assert 'declined' in requests
                assert not (home/'.nur/secrets.json').exists()
            else:
                assert 'the user saved $SERVICE_TOKEN' in requests
        else:
            command('Finish the migration')
            term.until('continue now', timeout=40)
            click('auto-continue on')
            term.until('auto-continue off')
            # Wheel movement cannot activate a chip or start a provider request.
            before = len(log.read_text().splitlines())
            term.write('\x1b[<64;30;15M\x1b[<65;30;15M')
            term.pump(.3)
            assert len(log.read_text().splitlines()) == before
            click('continue now')
            term.until('SECRET_FLOW_COMPLETED', timeout=30)
            assert 'Continue from where you left off' in log.read_text()
        print(f'PASS {scenario}', flush=True)
    finally:
        if term:
            (work/'terminal.txt').write_text(term.output, encoding='utf-8')
            (work/'screen.txt').write_text('\n'.join(term.screen.display), encoding='utf-8')
            term.close()
        server.shutdown()
        server.server_close()


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--bin')
    parser.add_argument('scenarios', nargs='*')
    args = parser.parse_args()
    for scenario in args.scenarios or ['secret-cancel', 'secret-click', 'secret-validation', 'secret-repeat',
                                      'secret-request', 'secret-request-cancel',
                                      'secret-small', 'secret-animation', 'secret-reduced-motion', 'resume-controls',
                                      'chime-enabled', 'chime-disabled', 'loop-repeat', 'loop-session',
                                      'admission-new-session', 'worker-background-cancel',
                                      'worker-background-session', 'swarm-question',
                                      'swarm-question-session', 'swarm-question-queue',
                                      'swarm-approval', 'swarm-approval-session']:
        run(find_binary(args.bin), scenario)

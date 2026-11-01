"""Real-terminal regressions for the v0.61 secrets and continuation UI.

Uses only an isolated home and scripted loopback provider. No clipboard access.
Run with --bin PATH [secret-cancel|secret-click|secret-repeat|resume-controls].
"""
import argparse
import json
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
        '[typesafe]\nenabled = false\n', encoding='utf-8')
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
        if scenario == 'secret-small':
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
                                      'secret-small', 'secret-animation', 'secret-reduced-motion', 'resume-controls']:
        run(find_binary(args.bin), scenario)

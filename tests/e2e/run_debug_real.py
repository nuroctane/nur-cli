"""Optional end-user DAP test against a real debugpy adapter."""
import argparse
import json
import sys
from pathlib import Path
from run_e2e import find_binary, run_scenario


def run(binary, debugpy):
    scenario = {
        'description': 'Drive the installed debugpy adapter against a real Python program.',
        'env': {'PYTHONPATH': str(Path(debugpy).resolve())},
        'setup_files': {'sample.py': 'x = 1\ny = x + 2\nprint(y)\n', 'pause.py': 'import time\ntime.sleep(2)\n'},
        'prompt': 'Debug sample.py, inspect threads, and stop.',
        'script': [
            {'tool_calls': [{'name':'debug', 'arguments': {'action':'start','session':'real','adapter':sys.executable,'adapter_args':['-m','debugpy.adapter'],'launch':{'program':'sample.py','console':'internalConsole'},'breakpoints':[{'path':'sample.py','lines':[2]}]}}]},
            {'tool_calls': [{'name':'bash', 'arguments': {'command':'python pause.py'}}]},
            {'tool_calls': [{'name':'debug', 'arguments': {'action':'events','session':'real'}}]},
            {'tool_calls': [{'name':'debug', 'arguments': {'action':'request','session':'real','command':'threads','arguments':{}}}]},
            {'tool_calls': [{'name':'debug', 'arguments': {'action':'stop','session':'real'}}]},
            {'text':'Real debugger completed.'},
        ],
        'expect': {'exit_code':0,'requests':6,'stdout_contains':['Real debugger completed.'], 'tool_results':[
            {'request':2,'contains':['real','verified']},
            {'request':4,'contains':['stopped','breakpoint']},
            {'request':5,'contains':['threads']},
            {'request':6,'contains':['stopped']},
        ]},
    }
    path = Path('.nur/scratch/real-debugpy-scenario.json')
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(scenario), encoding='utf-8')
    try:
        result = run_scenario(binary, path)
        if not result['passed']:
            raise AssertionError('; '.join(result['failures']))
        print('PASS debug-real-debugpy')
    finally:
        path.unlink(missing_ok=True)


if __name__ == '__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--bin');parser.add_argument('--debugpy',required=True)
    args=parser.parse_args();run(find_binary(args.bin),args.debugpy)

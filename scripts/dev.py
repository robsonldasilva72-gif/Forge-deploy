#!/usr/bin/env python3
"""Local process manager. Manages only PIDs recorded by this script; never removes data."""
import argparse
import getpass
import grp
import json
import os
from pathlib import Path
import shlex
import signal
import socket
import subprocess
import sys
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
RUNTIME = ROOT / '.forge'
STATE = RUNTIME / 'runtime.json'
OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def started(pid):
    try:
        fields = Path(f'/proc/{pid}/stat').read_text().rpartition(')')[2].split()
        return None if fields[0] == 'Z' else fields[19]
    except (FileNotFoundError, IndexError, ProcessLookupError):
        return None


def alive(record):
    return started(record['pid']) == record['started'] and record['started'] is not None


def load():
    return json.loads(STATE.read_text()) if STATE.exists() else {}


def docker_command(args):
    docker_group = grp.getgrnam('docker')
    if docker_group.gr_gid in os.getgroups() or os.geteuid() == 0:
        return args
    if getpass.getuser() in docker_group.gr_mem:
        return ['sg', 'docker', '-c', 'exec ' + shlex.join(args)]
    raise RuntimeError('Seu usuário precisa de acesso ao Docker. Consulte docs/setup-docker.md.')


def environment():
    env = os.environ.copy()
    if (ROOT / '.env').exists():
        for line in (ROOT / '.env').read_text().splitlines():
            line = line.strip()
            if line and not line.startswith('#') and '=' in line:
                key, value = line.split('=', 1)
                key = key.strip()
                if key.replace('_', '').isalnum():
                    env.setdefault(key, value.strip().strip('"\''))
    env.setdefault('DATABASE_URL', 'postgres://forge:forge_local@127.0.0.1:5432/forge')
    env.setdefault('FORGE_PORT', '3000')
    env.setdefault('RUST_LOG', 'info')
    return env


def stop():
    state = load()
    for name in ('worker', 'api'):
        record = state.get(name)
        if record and alive(record):
            try:
                os.killpg(record['pid'], signal.SIGTERM)
            except ProcessLookupError:
                continue
            deadline = time.monotonic() + 10
            while alive(record) and time.monotonic() < deadline:
                time.sleep(.1)
            if alive(record):
                os.killpg(record['pid'], signal.SIGKILL)
    STATE.unlink(missing_ok=True)
    print('API e executor encerrados. Aplicações, proxy e banco preservados.')


def status():
    state = load()
    if not state:
        print('Plataforma não iniciada por este script.')
        return
    print('Painel:', state.get('url', ''))
    for name in ('api', 'worker'):
        print(f'{name}:', 'ativo' if state.get(name) and alive(state[name]) else 'inativo')


def launch(name, args, env):
    with (RUNTIME / f'{name}.log').open('ab') as log:
        process = subprocess.Popen(args, cwd=ROOT, env=env, stdin=subprocess.DEVNULL,
                                   stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    return process, {'pid': process.pid, 'started': started(process.pid)}


def start(build):
    state = load()
    if any(alive(state[name]) for name in ('api', 'worker') if state.get(name)):
        raise RuntimeError('Já existe uma execução deste script. Use status ou stop antes de reiniciar.')
    env = environment()
    port = int(env['FORGE_PORT'])
    if not 1 <= port <= 65535:
        raise RuntimeError('FORGE_PORT deve estar entre 1 e 65535.')
    with socket.socket() as probe:
        probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        probe.bind(('127.0.0.1', port))
    subprocess.run(docker_command(['docker', 'compose', '-f', 'infra/compose.yaml', 'up', '-d', '--wait']), cwd=ROOT, check=True)
    if build or not (ROOT / 'crates/app/assets/pkg/forge_deploy_ui_bg.wasm').exists():
        subprocess.run(['bash', 'scripts/build-ui.sh'], cwd=ROOT, check=True)
    if build or not all((ROOT / f'target/debug/forge-deploy-{name}').exists() for name in ('app', 'worker')):
        subprocess.run(['cargo', 'build', '--workspace'], cwd=ROOT, check=True)
    RUNTIME.mkdir(exist_ok=True)
    state = {'url': f'http://127.0.0.1:{port}'}
    try:
        api, state['api'] = launch('api', [str(ROOT / 'target/debug/forge-deploy-app')], env)
        worker, state['worker'] = launch('worker', docker_command([str(ROOT / 'target/debug/forge-deploy-worker')]), env)
        STATE.write_text(json.dumps(state))
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if api.poll() is not None or worker.poll() is not None:
                raise RuntimeError('Um serviço encerrou. Consulte .forge/api.log e .forge/worker.log.')
            try:
                with OPENER.open(state['url'] + '/api/snapshot', timeout=2) as response:
                    if json.load(response)['worker_online']:
                        print('Plataforma pronta:', state['url'])
                        print('Logs: .forge/api.log e .forge/worker.log')
                        return
            except (OSError, ValueError):
                pass
            time.sleep(.2)
        raise RuntimeError('Não foi possível confirmar prontidão. Consulte os logs locais.')
    except BaseException:
        # Stop only services created by this invocation, never another application on the port.
        STATE.write_text(json.dumps(state))
        stop()
        raise


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['start', 'stop', 'status'])
    parser.add_argument('--build', action='store_true', help='recompilar UI e backend antes de iniciar')
    args = parser.parse_args()
    try:
        {'start': lambda: start(args.build), 'stop': stop, 'status': status}[args.action]()
    except (RuntimeError, OSError, subprocess.CalledProcessError) as error:
        print('Erro:', error, file=sys.stderr)
        sys.exit(1)

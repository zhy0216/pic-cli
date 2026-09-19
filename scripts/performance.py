#!/usr/bin/env python3
"""Linux E2E benchmark. Standard library only; every timed invocation is a new CLI process."""
import argparse
import collections
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import random
import shutil
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
SCHEMA = 1


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n')


def probe(args):
    try:
        p = subprocess.run(args, capture_output=True, text=True, check=False)
        return {'status': p.returncode, 'stdout': p.stdout, 'stderr': p.stderr}
    except OSError as error:
        return {'unavailable': str(error)}


def read(path):
    try:
        return Path(path).read_text()
    except OSError:
        return None


def machine(work):
    return {
        'platform': platform.platform(), 'python': platform.python_version(),
        'cpu': probe(['lscpu']), 'memory': read('/proc/meminfo'),
        'affinity': sorted(os.sched_getaffinity(0)), 'load': os.getloadavg(),
        'disk': probe(['df', '-T', str(work)]),
        'mount': probe(['findmnt', '-T', str(work)]),
        'block_devices': probe(['lsblk', '-o', 'NAME,TYPE,SIZE,ROTA,MODEL']),
        'cgroup': read('/proc/self/cgroup'),
        'cpu_max': read('/sys/fs/cgroup/cpu.max'),
        'memory_max': read('/sys/fs/cgroup/memory.max'),
        'rustc': probe(['rustc', '-Vv']), 'cargo': probe(['cargo', '-V']),
        'vips': probe(['vips', '--version']),
        'rss_sampler': probe(['/usr/bin/time', '--version']),
        'build_env': {k: os.environ.get(k) for k in
                      ['RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_TARGET',
                       'CARGO_PROFILE_RELEASE_OPT_LEVEL', 'CARGO_PROFILE_RELEASE_LTO',
                       'CARGO_PROFILE_RELEASE_CODEGEN_UNITS', 'RAYON_NUM_THREADS']},
        'build': 'cargo build --locked --release; default release profile; no added features',
        'cargo_toml_sha256': sha(ROOT / 'Cargo.toml'),
        'cargo_lock_sha256': sha(ROOT / 'Cargo.lock'),
        'git_head': probe(['git', '-C', str(ROOT), 'rev-parse', 'HEAD']),
        'git_diff': probe(['git', '-C', str(ROOT), 'diff', '--stat']),
    }


def invoke(binary, args, cwd):
    # GNU time waits for exactly its CLI child. A direct Python wait4 includes the Python
    # interpreter's inherited pre-exec RSS floor (~22 MiB here), inflating --version RSS.
    # File-backed stdout/stderr avoid pipe deadlocks and exclude JSON parsing from wall time.
    with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err, tempfile.TemporaryFile() as stats:
        start = time.perf_counter_ns()
        child = subprocess.Popen(['/usr/bin/time', '-f', '%M %U %S %R %F %I %O %w %c',
                                  '-o', f'/proc/self/fd/{stats.fileno()}', str(binary), *args],
                                 cwd=cwd, stdout=out, stderr=err, pass_fds=(stats.fileno(),))
        _, status, _ = os.wait4(child.pid, 0)
        wall = (time.perf_counter_ns() - start) / 1e6
        child.returncode = os.waitstatus_to_exitcode(status)
        out.seek(0)
        err.seek(0)
        stdout, stderr = out.read().decode(), err.read().decode()
        stats.seek(0)
        usage = stats.read().decode().split()
    if child.returncode != 0 or stderr:
        raise RuntimeError(f'{args}: exit={child.returncode}\n{stdout}\n{stderr}')
    result = None if args == ['--version'] else json.loads(stdout)
    if result is not None and not result.get('ok'):
        raise RuntimeError(stdout)
    if result is None and not stdout.startswith('pic-cli '):
        raise RuntimeError(stdout)
    return {'wall_ms': wall, 'peak_rss_kib': int(usage[0]),
            'user_ms': float(usage[1]) * 1000, 'system_ms': float(usage[2]) * 1000,
            'minor_faults': int(usage[3]), 'major_faults': int(usage[4]),
            'block_in': int(usage[5]), 'block_out': int(usage[6]),
            'voluntary_switches': int(usage[7]), 'involuntary_switches': int(usage[8]),
            'timings': result['timings'] if result else None}, result


def disk(path):
    logical = collections.Counter()
    allocated = 0
    for file in path.rglob('*'):
        if file.is_file():
            stat = file.stat()
            logical[file.relative_to(path).parts[0] if file.parent != path else 'metadata'] += stat.st_size
            allocated += stat.st_blocks * 512
    return {'logical_bytes': sum(logical.values()), 'allocated_bytes': allocated,
            'by_directory': dict(sorted(logical.items()))}


def op(name, params, target='canvas'):
    return {'op': name, 'op_version': 1, 'target': target, 'params': params}


def adjust(exposure=0.15, saturation=1.01):
    return {'exposure': exposure, 'brightness': 0, 'contrast': 1, 'saturation': saturation}


def pipelines(fixtures):
    resize = op('resize', {'width': 1280, 'height': None, 'filter': 'bilinear'})
    specs = {
        'photo': [resize, op('adjust', adjust(0.5, 1.1))],
        'adjust': [op('adjust', adjust(0.5, 1.1))],
        'blur': [op('blur', {'sigma': 12.0})],
        'layers': [op('layer_add', {'id': 'overlay', 'name': 'Overlay', 'source': '4k.png'}),
                   op('layer_set', {'opacity': 0.7, 'blend': 'screen'}, 'overlay')],
        # Twenty real, non-identity steps, retaining HDR, negative RGB and alpha semantics.
        'history': [op('adjust', adjust(0.15 if i % 2 == 0 else -0.12,
                                     1.01 if i % 2 == 0 else 0.99)) for i in range(20)],
    }
    for name, operations in specs.items():
        write_json(fixtures / f'{name}.json', {'schema_version': 1, 'operations': operations})


def benchmark(args):
    if args.runs < 30 and not args.smoke:
        raise ValueError('acceptance requires >=30 samples; use --smoke for a rehearsal')
    if args.cpu is not None:
        os.sched_setaffinity(0, {args.cpu})
    binary, fixture_tool = args.binary.resolve(), args.fixture_tool.resolve()
    work = args.work_dir.resolve()
    work.mkdir(parents=True, exist_ok=False)
    fixtures, outputs = work / 'fixtures', work / 'outputs'
    outputs.mkdir()
    subprocess.run([str(fixture_tool), 'generate', str(fixtures)], check=True)
    pipelines(fixtures)
    setup = []
    cache_budget = ['--cache-memory-bytes', str(1024**3), '--cache-disk-bytes', str(512 * 1024**2)]

    def cli(command, large=False):
        command = list(map(str, command)) + (cache_budget if large else []) + ['--json']
        sample, result = invoke(binary, command, work)
        setup.append({'argv': command, 'metrics': sample, 'data': result['data']})
        return result['data']

    def clone(source, dest):
        if dest.exists():
            shutil.rmtree(dest)
        shutil.copytree(source, dest)

    def checkpoint(project, revision, large=False):
        value = cli(['project', 'checkpoint', project, '--revision', revision], large)
        assert value['disk_stored'], value
        return value

    initial = work / 'initial.pic'
    cli(['project', 'create', '--input', fixtures / '1080.jpg', '--output', initial])
    history = work / 'history.pic'
    clone(initial, history)
    cli(['project', 'apply', history, '--pipeline', fixtures / 'history.json', '--expect-revision', 'r0'])
    checkpoint(history, 'r2')
    checkpoint(history, 'r17')
    checkpoint(history, 'r20')
    preview_args = ['project', 'preview', history, '--width', '640', '--filter', 'bilinear',
                    '--output', outputs / 'project_preview.png', '--png-compression', '6', '--overwrite']
    cli(preview_args)
    layered = work / 'layers.pic'
    cli(['project', 'create', '--input', fixtures / '4k.jpg', '--output', layered])
    cli(['project', 'apply', layered, '--pipeline', fixtures / 'layers.json', '--expect-revision', 'r0'])
    # Record the real default-budget fallback; never label it a checkpoint hit.
    default_checkpoint = cli(['project', 'checkpoint', layered])
    checkpoint(layered, 'r2', True)
    layer_preview = ['project', 'preview', layered, '--width', '960', '--filter', 'bilinear',
                     '--output', outputs / 'layers_preview.png', '--png-compression', '6', '--overwrite']
    cli(layer_preview, True)
    scratch = work / 'sample.pic'
    scenarios = []

    def add(name, command, *, source=None, clear=False, replay=None, large=False, output=None, store=False):
        command = list(map(str, command))
        if command != ['--version']:
            command += (cache_budget if large else []) + ['--json']
        scenarios.append(dict(name=name, argv=command, source=source, clear=clear, replay=replay,
                              large=large, output=output, store=store))

    add('version', ['--version'])
    for name, source, pipe, suffix in [
        ('jpeg1080_resize_adjust', '1080.jpg', 'photo', 'jpg'),
        ('jpeg4k_adjust', '4k.jpg', 'adjust', 'jpg'),
        ('png1080_transparent', '1080.png', 'photo', 'png'),
        ('layers4k', '4k.jpg', 'layers', 'png'),
        ('blur1080_sigma12', '1080.png', 'blur', 'png'),
    ]:
        output = outputs / f'{name}.{suffix}'
        add(name, ['run', '--input', fixtures / source, '--pipeline', fixtures / f'{pipe}.json',
                   '--output', output, '--overwrite',
                   *(['--jpeg-quality', '90'] if suffix == 'jpg' else ['--png-compression', '6'])],
            output=output)
    add('project_first_commit', ['project', 'apply', scratch, '--pipeline', fixtures / 'history.json',
                                '--expect-revision', 'r0'], source=initial, replay=(None, 0, 20))
    for name, project, source, clear, expected, large in [
        ('project_replay', scratch, history, True, (None, 0, 20), False),
        ('project_checkpoint', history, None, False, ('checkpoint', 20, 0), False),
        ('layers4k_replay', scratch, layered, True, (None, 0, 2), False),
        ('layers4k_checkpoint', layered, None, False, ('checkpoint', 2, 0), True),
    ]:
        output = outputs / f'{name}.png'
        add(name, ['project', 'export', project, '--output', output, '--png-compression', '6', '--overwrite'],
            source=source, clear=clear, replay=expected, large=large, output=output)
    add('project_preview_repeat', preview_args, replay=('preview', 20, 0), output=outputs / 'project_preview.png')
    add('layers4k_preview_repeat', layer_preview, replay=('preview', 2, 0), large=True,
        output=outputs / 'layers_preview.png')
    for step in [18, 3]:
        add(f'project_revise{step}', ['project', 'revise', scratch, '--step-revision', f'r{step}',
                                     '--params', json.dumps(adjust(0.25, 1.02)), '--expect-revision', 'r20'],
            source=history, replay=('checkpoint', step - 1, 21 - step))
    add('layers4k_checkpoint_store', ['project', 'checkpoint', scratch], source=layered, clear=True,
        replay=(None, 0, 2), large=True, store=True)
    if args.scenario:
        requested = set(args.scenario)
        assert requested <= {s['name'] for s in scenarios}, requested
        scenarios = [s for s in scenarios if s['name'] in requested]
    metadata = {
        'schema_version': SCHEMA, 'label': args.label, 'sample_count': args.runs,
        'started_utc': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
        'binary': str(binary), 'binary_sha256': sha(binary), 'fixture_tool_sha256': sha(fixture_tool),
        'work_dir': str(work),
        'runner_sha256': sha(Path(__file__)), 'machine': machine(work),
        'fixtures': {p.name: {'sha256': sha(p), 'bytes': p.stat().st_size} for p in sorted(fixtures.iterdir())},
        'cache_policy': 'one untimed warmup per scenario; OS pages uncontrolled/warm-biased, never dropped; '
                        'cache-clear removes only project-derived files; fresh CLI for every measurement',
        'timing_policy': 'perf_counter_ns around GNU time wrapper + CLI spawn/exit; excludes setup, '
                         'JSON parsing, tree accounting and output validation; no fsync guarantee',
        'rss_policy': 'GNU time wait4(CLI pid).ru_maxrss KiB; excludes Python/wrapper RSS; not RUSAGE_CHILDREN',
        'order': 'seed 11 shuffle of all selected scenarios in each round; one CLI at a time',
        'budgets': {'buffer_bytes': 1024**3, 'default_cache_memory_bytes': 256 * 1024**2,
                    'cache_disk_bytes': 512 * 1024**2, 'layer_checkpoint_cache_memory_bytes': 1024**3},
        'default_layer_checkpoint': default_checkpoint, 'setup': setup,
        'scenarios': [{k: str(v) if isinstance(v, Path) else v for k, v in s.items()} for s in scenarios],
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    meta_path = args.output.with_suffix('.meta.json')
    write_json(meta_path, metadata)
    rng = random.Random(11)
    output_hashes = {}
    with args.output.open('x') as stream:
        for iteration in range(args.runs + 1):
            ordered = scenarios.copy()
            rng.shuffle(ordered)
            for scenario in ordered:
                if scenario['source']:
                    clone(scenario['source'], scratch)
                    if scenario['clear']:
                        cli(['project', 'cache-clear', scratch], scenario['large'])
                command = scenario['argv']
                project = Path(command[2]) if command[:1] == ['project'] else None
                before = disk(project) if project else None
                if scenario['clear']:
                    assert all(before['by_directory'].get(d, 0) == 0 for d in ['checkpoints', 'cache']), before
                manifest_hash = sha(project / 'manifest.json') if project else None
                load = os.getloadavg()
                metrics, envelope = invoke(binary, command, work)
                data = envelope['data'] if envelope else {}
                replay = data.get('replay')
                if scenario['replay']:
                    kind, reused, recomputed = scenario['replay']
                    hit = replay['cache_hit']
                    assert (hit['kind'] if hit else None) == kind, replay
                    assert hit is None or hit['tier'] == 'disk', replay
                    assert replay['reused_steps'] == reused, replay
                    assert len(replay['recomputed_revisions']) == recomputed, replay
                if scenario['store']:
                    assert data['disk_stored'], data
                output = scenario['output']
                output_info = None
                if output:
                    digest = sha(output)
                    assert output_hashes.setdefault(scenario['name'], digest) == digest
                    output_info = {'sha256': digest, 'bytes': output.stat().st_size,
                                   'width': data['width'], 'height': data['height']}
                row = dict(schema_version=SCHEMA, label=args.label, scenario=scenario['name'],
                           iteration=iteration, phase='warmup' if iteration == 0 else 'sample',
                           utc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()), argv=command,
                           load_before=load, load_after=os.getloadavg(), **metrics, replay=replay,
                           disk_stored=data.get('disk_stored'), disk_before=before,
                           disk_after=disk(project) if project else None,
                           manifest_before_sha256=manifest_hash, output=output_info,
                           revision=data.get('revision'))
                stream.write(json.dumps(row, separators=(',', ':')) + '\n')
                stream.flush()
                print(f"{args.label} {iteration:02}/{args.runs} {scenario['name']}: "
                      f"{metrics['wall_ms']:.1f} ms, {metrics['peak_rss_kib']} KiB", flush=True)
    metadata['finished_utc'] = time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())
    metadata['machine_after'] = machine(work)
    # Includes all per-round cache-clear evidence; excluded from timed samples.
    write_json(meta_path, metadata)


def quantiles(values):
    values = sorted(values)
    return {'p50': values[math.ceil(len(values) * .50) - 1],
            'p95': values[math.ceil(len(values) * .95) - 1], 'max': values[-1]}


def summary(path):
    groups = collections.defaultdict(list)
    for line in path.read_text().splitlines():
        row = json.loads(line)
        if row['phase'] == 'sample':
            groups[row['scenario']].append(row)
    report = {}
    for name, rows in sorted(groups.items()):
        assert len({r['iteration'] for r in rows}) == len(rows)
        report[name] = {'n': len(rows), 'wall_ms': quantiles([r['wall_ms'] for r in rows]),
                        'peak_rss_kib': quantiles([r['peak_rss_kib'] for r in rows]),
                        'stages_ms': {k: quantiles([r['timings'][k] for r in rows])
                                      for k in (rows[0]['timings'] or {})},
                        'disk_after_bytes': quantiles([r['disk_after']['logical_bytes'] for r in rows])
                                            if rows[0]['disk_after'] else None,
                        'replay': rows[0]['replay']}
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    run = sub.add_parser('run')
    for flag in ['binary', 'fixture-tool', 'work-dir', 'output']:
        run.add_argument('--' + flag, type=Path, required=True)
    run.add_argument('--label', required=True)
    run.add_argument('--runs', type=int, default=30)
    run.add_argument('--cpu', type=int)
    run.add_argument('--smoke', action='store_true')
    run.add_argument('--scenario', action='append')
    summarize = sub.add_parser('summarize')
    summarize.add_argument('raw', type=Path, nargs='+')
    compare = sub.add_parser('compare')
    compare.add_argument('before', type=Path)
    compare.add_argument('after', type=Path)
    compare.add_argument('--max-p95-ratio', type=float,
                         help='opt-in local guard only; no default timing failure for shared CI')
    args = parser.parse_args()
    if args.command == 'run':
        benchmark(args)
    elif args.command == 'summarize':
        print(json.dumps({str(p): summary(p) for p in args.raw}, indent=2))
    else:
        before, after = summary(args.before), summary(args.after)
        assert before.keys() == after.keys(), 'scenario sets differ'
        a, b = [json.loads(p.with_suffix('.meta.json').read_text()) for p in [args.before, args.after]]
        assert a['fixtures'] == b['fixtures'], 'fixture or operation parameters changed'
        assert a['budgets'] == b['budgets'], 'cache budgets changed'
        ratios = {k: after[k]['wall_ms']['p95'] / before[k]['wall_ms']['p95'] for k in before}
        print(json.dumps({'p95_after_over_before': ratios, 'before': before, 'after': after}, indent=2))
        if args.max_p95_ratio is not None:
            assert all(v <= args.max_p95_ratio for v in ratios.values()), ratios


if __name__ == '__main__':
    main()

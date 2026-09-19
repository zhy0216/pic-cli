#!/usr/bin/env python3
"""Compare decoded outputs and every 20-step history boundary between two benchmark builds."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys

sys.dont_write_bytecode = True
from performance import adjust, invoke, sha, write_json


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--before', type=Path, required=True, help='before .meta.json')
    parser.add_argument('--after', type=Path, required=True, help='after .meta.json')
    parser.add_argument('--fixture-tool', type=Path, required=True)
    parser.add_argument('--work-dir', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    meta = [json.loads(p.read_text()) for p in [args.before, args.after]]
    tool = args.fixture_tool.resolve()
    work = args.work_dir.resolve()
    work.mkdir(parents=True, exist_ok=False)
    binaries = [Path(m['binary']) for m in meta]
    for binary, m in zip(binaries, meta):
        assert sha(binary) == m['binary_sha256']
    roots = [Path(m['work_dir']) for m in meta]
    evidence = []

    def compare(name, a, b):
        result = subprocess.run([str(tool), 'compare', str(a), str(b)], check=True,
                                capture_output=True, text=True)
        evidence.append({'name': name, 'left': str(a), 'right': str(b),
                         'left_sha256': sha(a), 'right_sha256': sha(b),
                         **json.loads(result.stdout)})

    def cli(index, command):
        _, result = invoke(binaries[index], list(map(str, command)) + ['--json'], roots[index])
        return result['data']

    # Every rendered workload, including direct, full replay, disk hit and preview.
    outputs = [dict((s['name'], s['output']) for s in m['scenarios'] if s['output']) for m in meta]
    assert outputs[0].keys() == outputs[1].keys()
    for name in outputs[0]:
        compare(name, outputs[0][name], outputs[1][name])
    for i in [0, 1]:
        compare(f'{i}_layer_direct_vs_replay', outputs[i]['layers4k'], outputs[i]['layers4k_replay'])
        compare(f'{i}_layer_replay_vs_checkpoint', outputs[i]['layers4k_replay'], outputs[i]['layers4k_checkpoint'])
        compare(f'{i}_history_replay_vs_checkpoint', outputs[i]['project_replay'], outputs[i]['project_checkpoint'])

    old_checkpoint = work / 'after-reading-before-checkpoint.png'
    restored = cli(1, ['project', 'export', roots[0] / 'layers.pic', '--output', old_checkpoint,
                       '--cache-memory-bytes', str(1024**3), '--cache-disk-bytes', str(512 * 1024**2),
                       '--png-compression', '6'])
    assert restored['replay']['cache_hit']['tier'] == 'disk'
    assert restored['replay']['reused_steps'] == 2
    assert restored['replay']['recomputed_revisions'] == []
    compare('after_reads_before_layer_checkpoint', outputs[0]['layers4k_checkpoint'], old_checkpoint)

    for step in range(21):
        images = [work / f'{i}-r{step}.png' for i in [0, 1]]
        inspections = []
        for i in [0, 1]:
            project = roots[i] / 'history.pic'
            cli(i, ['project', 'export', project, '--revision', f'r{step}', '--output', images[i],
                    '--png-compression', '6'])
            inspection = cli(i, ['project', 'inspect', project, '--revision', f'r{step}'])
            inspections.append({k: inspection[k] for k in
                                ['document', 'revision', 'current_revision', 'op_id', 'commit_id',
                                 'active_revisions', 'group_boundaries', 'commits']})
        assert inspections[0] == inspections[1], step
        compare(f'observable_r{step}', *images)
        for image in images:
            image.unlink()
    for step in [18, 3]:
        images = [work / f'{i}-revise{step}.png' for i in [0, 1]]
        for i in [0, 1]:
            project = work / f'{i}-revise{step}.pic'
            shutil.copytree(roots[i] / 'history.pic', project)
            change = cli(i, ['project', 'revise', project, '--step-revision', f'r{step}',
                             '--expect-revision', 'r20', '--params', json.dumps(adjust(0.25, 1.02))])
            assert change['replay']['reused_steps'] == step - 1
            assert len(change['replay']['recomputed_revisions']) == 21 - step
            cli(i, ['project', 'export', project, '--output', images[i], '--png-compression', '6'])
            cli(i, ['project', 'cache-clear', project])
            replayed = work / f'{i}-revise{step}-replay.png'
            cli(i, ['project', 'export', project, '--output', replayed, '--png-compression', '6'])
            compare(f'{i}_revise{step}_prefix_vs_full_replay', images[i], replayed)
        compare(f'revise{step}_before_after', *images)
    write_json(args.output, {'binary_sha256': [m['binary_sha256'] for m in meta],
                            'document_and_history_equal_at_revisions': list(range(21)),
                            'decoded_pixel_comparisons': evidence})


if __name__ == '__main__':
    main()

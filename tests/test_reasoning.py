import asyncio
import json
import sys
import threading
import time

import pytest
from textual.widgets import OptionList, Static, TabbedContent

from wy import agent_cli, reasoning, service
from wy.explorer import Explorer
from wy.storage import Store


def response(packet, *, citation=None):
    key = citation or packet['evidence'][0]['id']
    claim = {'text': 'The worker now uses a thread pool.', 'basis': 'observed', 'evidence_ids': [key]}
    return {'title': 'Understand the worker change', 'answer': claim, 'problem': claim,
            'before': claim, 'after': claim, 'steps': [claim], 'tradeoffs': [],
            'checks': [{'text': 'Measure I/O throughput.', 'basis': 'proposed', 'evidence_ids': [key]}],
            'unknowns': ['The workload is not measured.']}


def changed(repo):
    (repo / 'worker.py').write_text('pool = ThreadPoolExecutor(8)\n')
    return service.review(repo)


def test_packet_contains_real_diff_not_just_pattern_summary(repo):
    review = changed(repo)
    data = reasoning.packet(review, 'How does it change behavior?')
    diff = next(e for e in data['evidence'] if e['kind'] == 'diff')
    assert '-def work():' in diff['text']
    assert '+pool = ThreadPoolExecutor(8)' in diff['text']
    assert data['comparison_base']
    assert sum(len(e['text']) for e in data['evidence']) <= 100_000
    with pytest.raises(ValueError, match='current repository'):
        reasoning.packet(review, 'explain', '../other.py')


def test_new_assessment_keeps_original_decision_and_validates_citations(repo, monkeypatch):
    changed(repo)

    def invoke(agent, prompt, schema, **kwargs):
        data = json.loads(prompt.split('EVIDENCE PACKET:\n')[1])
        return response(data)

    monkeypatch.setattr(agent_cli, 'invoke', invoke)
    artifact = reasoning.run(repo, 'codex', history_source='none')
    assert artifact['context'] == 'separate_review'
    assert Store(repo).get('reasoning', 'latest')['id'] == artifact['id']
    assert Store(repo).latest().decisions[0].provenance == 'unexplained'
    monkeypatch.setattr(agent_cli, 'invoke', lambda *a, **kw: response(artifact['packet'], citation='invented'))
    with pytest.raises(ValueError, match='outside the supplied packet'):
        reasoning.run(repo, 'claude', history_source='none')
    assert Store(repo).get('reasoning', 'latest')['id'] == artifact['id']


def test_changes_during_reasoning_reject_the_result(repo, monkeypatch):
    changed(repo)

    def invoke(agent, prompt, schema, **kwargs):
        data = json.loads(prompt.split('EVIDENCE PACKET:\n')[1])
        (repo / 'worker.py').write_text('pool = ThreadPoolExecutor(12)\n')
        return response(data)

    monkeypatch.setattr(agent_cli, 'invoke', invoke)
    with pytest.raises(ValueError, match='Repository changed'):
        reasoning.run(repo, 'codex', history_source='none')
    with pytest.raises(ValueError, match='No reasoning'):
        Store(repo).get('reasoning', 'latest')


def test_cli_commands_disable_modification_tools_and_persistence(tmp_path, monkeypatch):
    monkeypatch.setattr(agent_cli.shutil, 'which', lambda name: '/installed/' + name)
    codex = agent_cli.command('codex', tmp_path, {'type': 'object'})
    assert codex[codex.index('--sandbox') + 1] == 'read-only'
    assert '--ephemeral' in codex and '--ignore-user-config' in codex
    assert 'shell_tool' in codex and 'hooks' in codex and 'multi_agent' in codex
    claude = agent_cli.command('claude', tmp_path, {'type': 'object'})
    assert claude[claude.index('--tools') + 1] == ''
    assert '--strict-mcp-config' in claude and '--no-session-persistence' in claude
    assert '--dangerously-skip-permissions' not in claude


def test_subprocess_timeout_and_cancel_stop_child(monkeypatch):
    monkeypatch.setattr(agent_cli, 'command', lambda *a: [sys.executable, '-c', 'import time; time.sleep(30)'])
    before = time.monotonic()
    with pytest.raises(ValueError, match='timeout'):
        agent_cli.invoke('codex', '{}', {}, timeout=0.1)
    assert time.monotonic() - before < 4
    cancel = threading.Event()
    cancel.set()
    with pytest.raises(ValueError, match='cancelled'):
        agent_cli.invoke('codex', '{}', {}, cancel=cancel)


@pytest.mark.parametrize('agent', ['codex', 'claude'])
def test_agent_output_decoding_and_redaction(tmp_path, monkeypatch, agent):
    def command(name, directory, schema):
        result = {'text': 'password="hidden-value"'}
        script = "import json,sys;sys.stdin.read();"
        if name == 'codex':
            script += f"from pathlib import Path;Path({str(directory / 'response.json')!r}).write_text({json.dumps(result)!r})"
        else:
            script += f"print({json.dumps({'structured_output': result})!r})"
        return [sys.executable, '-c', script]

    monkeypatch.setattr(agent_cli, 'command', command)
    result = agent_cli.invoke(agent, '{}', {})
    assert 'hidden-value' not in result['text']


def test_understand_view_runs_cli_and_opens_exact_citation(repo, monkeypatch):
    review = changed(repo)
    monkeypatch.setattr(agent_cli, 'invoke', lambda agent, prompt, schema, **kwargs: response(json.loads(prompt.split('EVIDENCE PACKET:\n')[1])))

    async def navigate():
        workspace = Explorer(review)
        workspace.source_mode = 'none'
        async with workspace.run_test(size=(150, 50)) as pilot:
            await pilot.pause()
            assert workspace.query_one('#tabs', TabbedContent).active == 'reason-tab'
            await pilot.click('#reason-now')
            await workspace.workers.wait_for_complete()
            await pilot.pause()
            assert workspace.reason_artifact
            assert workspace.reason_citations
            assert 'Understand the worker change' in str(workspace.query_one('#reason-copy', Static).render())
            options = workspace.query_one('#reason-citations', OptionList)
            options.focus()
            options.highlighted = 0
            await pilot.pause()
            await pilot.press('enter')
            assert workspace.query_one('#tabs', TabbedContent).active == 'reason-evidence-tab'

    asyncio.run(navigate())

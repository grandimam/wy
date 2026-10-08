import asyncio

from textual.widgets import Input, Select, TextArea

from wy import service, source_view
from wy.explorer import Explorer


def test_structural_outline_and_bounded_window():
    source = 'class Worker:\n    async def run(self):\n        return 1\n\ndef other():\n    return 2\n'
    symbols = source_view.outline('worker.py', source)
    assert [(s.name, s.start, s.end) for s in symbols] == [('Worker', 1, 3), ('Worker.run', 2, 3), ('other', 5, 6)]
    assert source_view.window(source, symbols, 2)[2] == 'function Worker.run'
    large = 'def huge():\n' + '    x = 1\n' * 300
    symbols = source_view.outline('worker.py', large)
    start, end, _ = source_view.window(large, symbols, 180)
    assert start <= 180 <= end
    assert end - start + 1 <= 100


def test_outline_for_documents_and_unsupported_files():
    source = '# Intro\ntext\n```python\n# not a heading\n```\n## Details\ntext\n# End\n'
    symbols = source_view.outline('README.md', source)
    assert [s.name for s in symbols] == ['Intro', 'Details', 'End']
    assert symbols[0].end == 7
    assert source_view.outline('plain.txt', 'anything') == []
    start, end, _ = source_view.window('x\n' * 400, [], 200)
    assert start < 200 < end and end - start < 30
    assert [s.name for s in source_view.outline('package.json', '{"name":"wy","scripts":{"test":"pytest"}}')] == ['"name"', '"scripts"', '"scripts"."test"']


def test_focused_view_outline_full_toggle_and_line_mapping(repo):
    text = 'def work():\n    pool = ThreadPoolExecutor(8)\n    return pool\n'
    text += '\n' * 120
    text += 'def later():\n    return "needle"\n'
    (repo / 'worker.py').write_text(text)
    review = service.review(repo)
    decision = review.decisions[0]
    citation = decision.evidence[0]
    original = (repo / 'worker.py').read_bytes()

    async def navigate():
        workspace = Explorer(review)
        async with workspace.run_test(size=(145, 48)) as pilot:
            workspace.navigate(('evidence', decision.id, citation.id))
            await pilot.pause()
            area = workspace.query_one('#current-code', TextArea)
            assert area.document.line_count < 30
            assert 'ThreadPoolExecutor' in area.text
            assert 'needle' not in area.text
            assert area.selected_text
            workspace.toggle_full()
            assert 'needle' in area.text
            assert area.line_number_start == 1
            workspace.toggle_full()
            later = next(i for i, s in enumerate(workspace.source_symbols) if s.name == 'later')
            workspace.query_one('#file-outline', Select).value = str(later)
            await pilot.pause()
            assert 'needle' in area.text
            assert area.line_number_start > 100
            workspace.action_back()
            await pilot.pause()
            assert 'ThreadPoolExecutor' in area.text
            workspace.query_one('#source-search', Input).value = 'needle'
            workspace.find_next()
            assert 'needle' in area.text
            row, _ = area.cursor_location
            assert row + area.line_number_start == 125
            field = workspace.query_one('#line-input', Input)
            workspace.goto_line(Input.Submitted(field, '2'))
            row, _ = area.cursor_location
            assert row + area.line_number_start == 2
            assert 'ThreadPoolExecutor' in area.text
            workspace.focus_citation()
            assert workspace.routes[workspace.route_index] == ('evidence', decision.id, citation.id)
            await pilot.resize_terminal(85, 32)
            await pilot.pause()

    asyncio.run(navigate())
    assert (repo / 'worker.py').read_bytes() == original

"""Executable checks for public documentation examples."""

from __future__ import annotations

import re
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
DOC_PATHS = [
    ROOT / "README.md",
    ROOT / "docs" / "src" / "CALENDARS.md",
    ROOT / "docs" / "src" / "API.md",
]


@pytest.mark.parametrize("doc_path", DOC_PATHS, ids=lambda path: str(path.relative_to(ROOT)))
def test_python_doc_examples_execute(doc_path: Path) -> None:
    text = doc_path.read_text(encoding="utf-8")
    blocks = re.findall(r"```python\n(.*?)```", text, flags=re.S)
    namespace = {"__name__": f"__docs_{doc_path.stem}__"}

    for index, block in enumerate(blocks, start=1):
        exec(compile(block, f"{doc_path.relative_to(ROOT)} block {index}", "exec"), namespace)

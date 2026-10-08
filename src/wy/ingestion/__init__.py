"""Agent adapters normalize observable events; private reasoning is never ingested."""

from pathlib import Path
from typing import Protocol

from wy.models import Session


class SessionCollector(Protocol):
    def collect(self, path: Path) -> Session: ...

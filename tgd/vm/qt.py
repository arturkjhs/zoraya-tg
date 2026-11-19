"""The part of PySide6.QtCore that tgclient's view-models use, without Qt.

tgclient's models and controllers keep their method bodies; only their imports change from
`PySide6.QtCore` to this module. What Qt would tell a view, this module tells whoever
connected: a `Signal` is a list of callables, and a `QAbstractListModel`'s begin/end pairs
emit `rowsInserted`, `rowsRemoved`, `rowsMoved` and `modelReset` as Qt does, so the protocol
server (`server.py`) listens to the same notifications QML did.

Nothing here runs an event loop: tgclient's models already schedule their work on asyncio.
"""

from __future__ import annotations

from collections.abc import Callable
from enum import IntEnum
from typing import Any


class _Bound:
    """A signal of one object: `connect`, `disconnect`, `emit`."""

    __slots__ = ("_slots",)

    def __init__(self) -> None:
        self._slots: list[Callable[..., Any]] = []

    def connect(self, slot: Callable[..., Any]) -> None:
        self._slots.append(slot)

    def disconnect(self, slot: Callable[..., Any] | None = None) -> None:
        if slot is None:
            self._slots.clear()
        elif slot in self._slots:
            self._slots.remove(slot)

    def emit(self, *args: Any) -> None:
        for slot in list(self._slots):
            slot(*args)


class Signal:
    """A class attribute in tgclient (`changed = Signal()`); per object, a `_Bound`."""

    def __init__(self, *types: Any, **_: Any) -> None:
        self._name = ""

    def __set_name__(self, owner: type, name: str) -> None:
        self._name = "_signal_" + name

    def __get__(self, obj: Any, owner: type | None = None) -> Any:
        if obj is None:
            return self
        bound = obj.__dict__.get(self._name)
        if bound is None:
            bound = obj.__dict__[self._name] = _Bound()
        return bound


def Slot(*_types: Any, **_: Any) -> Callable[[Callable[..., Any]], Callable[..., Any]]:  # noqa: N802
    return lambda f: f


def Property(_type: Any, fget: Callable[..., Any] | None = None,  # noqa: N802
             fset: Callable[..., Any] | None = None, **_: Any) -> Any:
    """`@Property(str, notify=x)` over a getter, or `Property(str, get, set, notify=x)`."""
    if fget is None:
        return property
    return property(fget, fset)


class _ItemDataRole(IntEnum):
    DisplayRole = 0
    UserRole = 256


class Qt:
    ItemDataRole = _ItemDataRole


QByteArray = bytes


class QModelIndex:
    __slots__ = ("_row",)

    def __init__(self, row: int = -1) -> None:
        self._row = row

    def isValid(self) -> bool:  # noqa: N802
        return self._row >= 0

    def row(self) -> int:
        return self._row

    def __eq__(self, other: object) -> bool:
        return isinstance(other, QModelIndex) and other._row == self._row

    def __hash__(self) -> int:
        return hash(self._row)


QPersistentModelIndex = QModelIndex


class QObject:
    def __init__(self, parent: Any = None) -> None:
        self._parent = parent


class QAbstractListModel(QObject):
    """Qt's list model contract: the begin/end calls bracket a change, and the end call
    tells the listeners what changed, with Qt's arguments (rows inclusive)."""

    dataChanged = Signal()  # (top: QModelIndex, bottom: QModelIndex, roles: list[int])
    rowsInserted = Signal()  # (parent, first, last)
    rowsRemoved = Signal()  # (parent, first, last)
    rowsMoved = Signal()  # (parent, first, last, destination parent, destination row)
    modelReset = Signal()

    def __init__(self, parent: Any = None) -> None:
        super().__init__(parent)
        self._pending: list[tuple[Any, ...]] = []

    def index(self, row: int, column: int = 0, parent: Any = None) -> QModelIndex:
        return QModelIndex(row)

    def beginInsertRows(self, parent: Any, first: int, last: int) -> None:  # noqa: N802
        self._pending.append(("insert", first, last))

    def endInsertRows(self) -> None:  # noqa: N802
        _, first, last = self._pending.pop()
        self.rowsInserted.emit(QModelIndex(), first, last)

    def beginRemoveRows(self, parent: Any, first: int, last: int) -> None:  # noqa: N802
        self._pending.append(("remove", first, last))

    def endRemoveRows(self) -> None:  # noqa: N802
        _, first, last = self._pending.pop()
        self.rowsRemoved.emit(QModelIndex(), first, last)

    def beginMoveRows(self, parent: Any, first: int, last: int, destination_parent: Any,  # noqa: N802
                      destination: int) -> bool:
        self._pending.append(("move", first, last, destination))
        return True

    def endMoveRows(self) -> None:  # noqa: N802
        _, first, last, destination = self._pending.pop()
        self.rowsMoved.emit(QModelIndex(), first, last, QModelIndex(), destination)

    def beginResetModel(self) -> None:  # noqa: N802
        self._pending.append(("reset",))

    def endResetModel(self) -> None:  # noqa: N802
        self._pending.pop()
        self.modelReset.emit()

    def rowCount(self, parent: Any = None) -> int:  # noqa: N802
        return 0

    def data(self, index: Any, role: int = 0) -> Any:
        return None

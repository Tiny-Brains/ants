"""The architectures, one per weight class.

Three things shape every net here, and none of them is "what usually works on images":

**The board wraps.** Ants maps are toroidal, so a zero-padded convolution tells the model there is a
wall at the edge where there is in fact the other side of the map. Every convolution here is padded
by wrapping, built from `cat` of edge slices — which exports to `Slice` + `Concat`, both on the
operator allowlist, where `padding_mode="circular"` exports to a `Pad` mode that only exists from
opset 18.

**The board has many sizes.** Anything from 24 to 124 a side (the cartridge's `limits.boards`), and a
season's boards change while it runs. Fully convolutional handles that for free, and it is the main
reason to stay fully convolutional. Where a net downsamples (`EncDec`), the stride must divide
every side it is run on: the basic boards' sides all divide by four, and many of season 1's do not
(26, 45, 49, 105 ...), so a strided net needs padding to the stride before it is fit to submit.

**Above `mini` the turn deadline binds before the byte cap does.** See `classes.toml` `[compute]`.
So the ladder is not one architecture scaled up: `trunk` spends its parameters at full resolution,
`encdec` moves the bulk to a stride where a parameter costs a quarter or a sixteenth as much.

## Receptive field, not parameter count, was the binding constraint

The first models trained here were `Trunk(channels=48, blocks=1)` — one 3x3 stem, one 3x3 block,
then 1x1s. That is a **5x5 receptive field**: an ant could see two cells in each direction, and the
teacher it was imitating plans over a flood 32 cells deep. Both nano and micro plateaued within one
epoch at 44-47% agreement and played far below the teacher, growing colonies of 5 and 15 ants where
the teacher grows 33 — and micro's extra parameters bought almost nothing, because the thing it
lacked was not capacity.

Dilation fixes it for free. `Conv` carries dilation as an attribute, so nothing new is allowlisted,
and doubling it each layer reaches 31x31 in four layers where an undilated stack would need sixteen.

The value head is **training only and never exported**. It is a genuine saving — at nano it would be
a third of the budget — and it is also the honest place to put privileged input: a critic may see
the true score (`tinybrains env --scores every`), because a critic is discarded before anything
plays. The policy sees the fog-filtered view and nothing else.

## Padding is done once a stage, not once a layer

The first version of this file wrapped before every 3x3 and measured **2.3x slower than the FLOP
count predicted** — three convolutions meant six full-tensor concats. A stack of `n` 3x3
convolutions with no padding shrinks the board by `2n`, so one wrap of `n` cells up front leaves
exactly the original size at the end, for one pair of concats and about 10% more interior work at
128x128. Same arithmetic, same result, a third of the copying.
"""

from __future__ import annotations

import torch
import torch.nn as nn
import torch.nn.functional as F

from .planes import (ANT_MEMORY, MEMORY_SOURCES, MISSION, N_MEMORY, N_MOVES, N_PLANES, PLANES,
                     XATHIS_MEMORY, XATHIS_REACH, XATHIS_STAY_CAP)


def wrap(x: torch.Tensor, k: int) -> torch.Tensor:
    """Toroidal padding of `k` cells on all four sides, as `cat` of edge slices.

    Not `F.pad(mode="circular")`: that exports to ONNX `Pad` with `wrap`, which is opset 18, and the
    deployment's allowlist is checked per operator rather than per mode. Slice and Concat are
    unambiguous, and both are listed.
    """
    if k == 0:
        return x
    x = torch.cat([x[..., -k:], x, x[..., :k]], dim=-1)
    x = torch.cat([x[..., -k:, :], x, x[..., :k, :]], dim=-2)
    return x


class Stage(nn.Module):
    """A run of unpadded 3x3 convolutions, wrapped once at the front.

    The board comes out the size it went in, because a 3x3 at dilation `d` shrinks it by `2d` and
    the wrap added `sum(dilations)` on every side.

    **Dilation is why this class exists.** See the module docstring: a stack of ordinary 3x3s grows
    its receptive field by two cells a layer, and the first models trained here could see two cells
    and played like it. Doubling the dilation each layer grows it exponentially instead — 1, 2, 4, 8
    reaches 31x31 in four layers — and costs nothing but the wider wrap. ONNX carries dilation as an
    attribute of `Conv` rather than as its own operator, so it needs nothing the allowlist does not
    already have: a dilated graph inspects as `Cast, Concat, Constant, Conv, Relu, Slice`.
    """

    def __init__(self, widths: list[tuple[int, int]], dilations: list[int] | None = None):
        super().__init__()
        self.dilations = dilations or [1] * len(widths)
        assert len(self.dilations) == len(widths)
        self.convs = nn.ModuleList([
            nn.Conv2d(a, b, 3, padding=0, dilation=d)
            for (a, b), d in zip(widths, self.dilations)
        ])

    @property
    def reach(self) -> int:
        """How far a cell of the output can see, in cells. The number that decides whether a policy
        can follow a trail or only feel the square it is on."""
        return sum(self.dilations)

    def forward(self, x):
        x = wrap(x, self.reach)
        for i, c in enumerate(self.convs):
            x = c(x)
            if i + 1 < len(self.convs):
                x = F.relu(x)
        return F.relu(x)


def doubling(n: int, cap: int = 8) -> list[int]:
    """1, 2, 4, 8, 8, ... — dilations for `n` layers, stopped doubling at `cap`.

    Past 8 the holes are wider than the food is dense and the kernel starts sampling noise, so the
    tail repeats rather than continuing to double. Four layers reach 15 cells each way, which is
    about the view radius (radius^2 77, so 8.8 cells) plus room to head somewhere.
    """
    return [min(cap, 1 << i) for i in range(n)]


class Trunk(nn.Module):
    """Fully convolutional at full resolution: nano and micro.

    One dilated 3x3 stage, then 1x1 layers, which buy depth for a ninth of what a 3x3 costs — the
    right trade when the whole budget is a few thousand parameters. No normalisation layer: at these
    widths a norm's parameters are a visible fraction of the budget, and the planes are already 0/1.

    `dilate=False` reproduces the undilated stack the first models used. It is kept because the
    comparison between them is the finding, not a footnote.
    """

    def __init__(self, channels: int, blocks: int, planes: int = N_PLANES, moves: int = N_MOVES,
                 dilate: bool = True):
        super().__init__()
        widths = [(planes, channels)] + [(channels, channels)] * blocks
        dilations = doubling(len(widths)) if dilate else None
        self.stage = Stage(widths, dilations)
        self.mix = nn.Conv2d(channels, channels, 1)
        self.head = nn.Conv2d(channels, moves, 1)
        self.channels = channels

    def features(self, x):
        return F.relu(self.mix(self.stage(x)))

    def forward(self, board):
        return self.head(self.features(board.float()))


class EncDec(nn.Module):
    """Stride the middle, spend the bytes there, come back up: mini and small.

    A narrow full-resolution stem keeps per-cell detail and stays cheap; the bulk runs at `stride`,
    where a parameter costs 1/stride² of a full-resolution one; then nearest-upsample and
    concatenate the stem back so the 1x1 head still sees the cell it is deciding for.

    `Resize` is the upsampler because **`ConvTranspose` is not on the operator allowlist**, and
    `avg_pool` is the downsampler for the same reason a stride-2 convolution would also work but
    costs parameters this class would rather spend at depth.
    """

    def __init__(
        self, channels: int, stride: int, blocks: int,
        planes: int = N_PLANES, moves: int = N_MOVES,
    ):
        super().__init__()
        assert stride in (2, 4), "a stride must divide every side the net is run on"
        self.stride = stride
        stem_ch = max(8, channels // 4)
        # The stem stays undilated: it runs at full resolution and its job is per-cell detail. Reach
        # is the deep stack's job, and down there one cell is `stride` cells of board already.
        self.stem = Stage([(planes, stem_ch)])
        deep_widths = [(stem_ch, channels)] + [(channels, channels)] * blocks
        self.deep = Stage(deep_widths, doubling(len(deep_widths), cap=4))
        self.up = nn.Conv2d(channels, stem_ch, 1)
        self.channels = stem_ch * 2
        self.head = nn.Conv2d(self.channels, moves, 1)

    def features(self, x):
        skip = self.stem(x)
        y = self.deep(F.avg_pool2d(skip, self.stride))
        y = F.relu(self.up(y))
        y = F.interpolate(y, scale_factor=self.stride, mode="nearest")
        return torch.cat([skip, y], dim=1)

    def forward(self, board):
        return self.head(self.features(board.float()))


class PerCell(nn.Module):
    """1x1 convolutions only: the same capacity with **no receptive field at all**.

    This is the method column's control. It sees exactly the seven values at the cell its ant stands
    on — is there water here, a hill, an ant of mine — and nothing about what is next to it, so it
    cannot follow a food trail, avoid a fight or head for a frontier. Give it the same class, the
    same dataset and roughly the same parameter count as `Trunk` and the difference between them is
    the receptive field and nothing else, which is the only way to say what spatial context is worth
    in this game rather than to assert it.

    A per-cell model is also the honest floor for the whole enterprise. If a convolutional trunk
    cannot beat one, the trunk is not learning to look around.
    """

    def __init__(self, channels: int, blocks: int = 1, planes: int = N_PLANES,
                 moves: int = N_MOVES):
        super().__init__()
        widths = [(planes, channels)] + [(channels, channels)] * blocks
        self.layers = nn.ModuleList([nn.Conv2d(a, b, 1) for a, b in widths])
        self.head = nn.Conv2d(channels, moves, 1)
        self.channels = channels

    def features(self, x):
        for layer in self.layers:
            x = F.relu(layer(x))
        return x

    def forward(self, board):
        return self.head(self.features(board.float()))


class WithMemory(nn.Module):
    """A trunk that reads the board and the seat's memory, and writes the memory back.

    The memory is `planes.MEMORY`: two 0/1 planes the view forgets between turns, kept with `Max`.
    It is a fixed function of what the seat has seen, so nothing here is learned but the policy
    that reads it. The trunk is built with `N_PLANES + N_MEMORY` input channels and sees the board
    and the memory stacked. The update runs in float and casts back, so the graph carries `Max`
    over floats (the variant every runtime executes) and hands back `i8`, the dtype the manifest
    declares and the board already uses. `export.to_onnx` names the ports `board`, `memory_in`,
    `policy` and `memory`, which is what the generated manifest declares.
    """

    memory_ports = True     # exported with `memory_in` and `memory` beside `board` and `policy`
    memory_kind = "max"

    def __init__(self, trunk: nn.Module):
        super().__init__()
        self.trunk = trunk
        self.channels = trunk.channels
        self.register_buffer("sources", torch.tensor(MEMORY_SOURCES, dtype=torch.long))

    def read(self, memory_in: torch.Tensor) -> torch.Tensor:
        """The planes the trunk sees for the memory."""
        return memory_in.float()

    def write(self, f: torch.Tensor, board: torch.Tensor, memory_in: torch.Tensor) -> torch.Tensor:
        """The memory for the next turn."""
        seen = board.index_select(1, self.sources).float()
        return torch.maximum(memory_in.float(), seen).to(torch.int8)

    def forward(self, board: torch.Tensor, memory_in: torch.Tensor):
        f = self.trunk.features(torch.cat([board.float(), self.read(memory_in)], dim=1))
        return self.trunk.head(f), self.write(f, board, memory_in)


class LearnedMemory(nn.Module):
    """A trunk whose memory the graph learns: `N_MEMORY` planes written each turn from what the
    seat sees and what it remembered, carried as `i8` exactly as `WithMemory`'s are.

    Where `WithMemory` keeps two named planes with `Max`, this keeps two the optimiser shapes -- a
    convolutional GRU whose input transform is the trunk itself. `memory_in` arrives as `i8` in
    [-127, 127] and is read as `h = memory_in / 127`; the trunk sees the board and `h` stacked; a
    1x1 gate and a 1x1 candidate over the features and `h` give `h' = (1 - z) h + z tanh(c)`; and
    the output is `Floor(h' x 127 + 0.5)` cast to `i8`, which is what the runner carries. `Round` is
    not on the operator allowlist; `Floor`, `Cast`, `Sigmoid` and `Tanh` are.

    **Training carries the rounded value too.** The forward pass rounds and the backward pass sees
    the identity (a straight-through estimator), so what the net learns to read back on the next
    turn is exactly what a runner will hand it, and nothing is learned about a precision the wire
    does not carry. The manifest is `WithMemory`'s: two planes of `i8`, 2 bytes a cell, so a class
    that allows one allows the other, and `train/seq.py` is the trainer, because a memory the graph
    computes has to be replayed in a seat's turn order.
    """

    memory_ports = True
    memory_kind = "learned"

    def __init__(self, trunk: nn.Module, planes: int = N_MEMORY):
        super().__init__()
        self.trunk = trunk
        self.channels = trunk.channels
        self.planes = planes
        self.gate = nn.Conv2d(trunk.channels + planes, planes, 1)
        self.cand = nn.Conv2d(trunk.channels + planes, planes, 1)

    def read(self, memory_in: torch.Tensor) -> torch.Tensor:
        return memory_in.float() / 127.0

    def write(self, f: torch.Tensor, board: torch.Tensor, memory_in: torch.Tensor) -> torch.Tensor:
        h = self.read(memory_in)
        fh = torch.cat([f, h], dim=1)
        z = torch.sigmoid(self.gate(fh))
        c = torch.tanh(self.cand(fh))
        scaled = ((1 - z) * h + z * c) * 127.0
        rounded = torch.floor(scaled + 0.5)
        if self.training:
            # The value carried forward is the rounded one; the gradient is the unrounded one's.
            return scaled + (rounded - scaled).detach()
        return rounded.to(torch.int8)

    def forward(self, board: torch.Tensor, memory_in: torch.Tensor):
        f = self.trunk.features(torch.cat([board.float(), self.read(memory_in)], dim=1))
        return self.trunk.head(f), self.write(f, board, memory_in)


def roll(x: torch.Tensor, shift: int, dim: int) -> torch.Tensor:
    """A shift of one cell on a board axis, wrapping: the two slices and the concat it exports to,
    with constant bounds so the graph reads no shape (`Slice` and `Concat`, nothing more)."""
    assert dim in (2, 3) and shift in (1, -1)
    if dim == 2:
        return torch.cat([x[:, :, -1:], x[:, :, :-1]], dim=2) if shift == 1 else torch.cat([x[:, :, 1:], x[:, :, :1]], dim=2)
    return torch.cat([x[:, :, :, -1:], x[:, :, :, :-1]], dim=3) if shift == 1 else torch.cat([x[:, :, :, 1:], x[:, :, :, :1]], dim=3)


PLANE = {p.name: i for i, p in enumerate(PLANES)}


class XathisMemory(nn.Module):
    """The 2011 winner's two board memories, as the graph computes them (`planes.py`, *the 2011
    winner's memory*). Nothing here is learned: `read` turns the two `u8` planes into what the
    trunk sees, and `write` is the bot's update, its ten-step walk from every own ant and hill
    through land (`reach="walk"`) or, where a class cannot afford those nodes, the view itself
    (`reach="sight"`), and its stillness detector over the enemies in view.

    Both are written as convolutions with fixed kernels over a board wrapped once, because a graph
    is priced by its bytes: a step of the walk is one 3x3 cross `Conv` (the four neighbours), a
    `Clip` and a `Mul` by the land, against the twelve `Slice` and `Concat` nodes a shift a side
    would cost; the neighbour mask is one `Conv` whose kernel holds 1, 2, 4 and 8 at N, E, S, W.
    """

    memory_ports = True
    memory_kind = "xathis"
    dtype = torch.uint8

    def __init__(self, trunk: nn.Module, reach: str = "walk"):
        super().__init__()
        self.trunk = trunk
        self.channels = trunk.channels
        self.reach = reach
        cross = torch.tensor([[0.0, 1.0, 0.0], [1.0, 1.0, 1.0], [0.0, 1.0, 0.0]]).reshape(1, 1, 3, 3)
        # The mask kernel reads the neighbour's plane: a foe NORTH of the cell is at row -1.
        mask = torch.tensor([[0.0, 1.0, 0.0], [8.0, 0.0, 2.0], [0.0, 4.0, 0.0]]).reshape(1, 1, 3, 3)
        self.register_buffer("cross", cross)
        self.register_buffer("mask_kernel", mask)

    @staticmethod
    def planes() -> int:
        """What the trunk sees for the memory: explore scaled, the stay count scaled, will-stay."""
        return 3

    def read(self, memory_in: torch.Tensor) -> torch.Tensor:
        m = memory_in.float()
        explore = m[:, 0:1] / 255.0
        stay = m[:, 1:2]
        has = (stay >= 128).float()
        count = torch.floor((stay - 128.0) / 16.0) * has
        return torch.cat([explore, count / XATHIS_STAY_CAP, (count >= 5).float()], dim=1)

    def write(self, f: torch.Tensor, board: torch.Tensor, memory_in: torch.Tensor) -> torch.Tensor:
        b = board.float()
        water = b[:, PLANE["water"]:PLANE["water"] + 1]
        m = memory_in.float()
        explore, stay = m[:, 0:1], m[:, 1:2]
        if self.reach == "walk":
            land = 1.0 - water
            src = torch.clamp(b[:, PLANE["mine"]:PLANE["mine"] + 1] + b[:, PLANE["hill_mine"]:PLANE["hill_mine"] + 1], 0, 1) * land
            # Wrapped once, a cell wider than the walk: each step is a cross convolution padded
            # with zeros, which is wrong only at the outermost ring and creeps inward a cell a
            # step, so the margin keeps the board itself exact and is cropped off at the end.
            k = XATHIS_REACH + 1
            reach = wrap(src, k)
            land_w = wrap(land, k)
            for _ in range(XATHIS_REACH):
                reach = torch.clamp(F.conv2d(reach, self.cross, padding=1), 0, 1) * land_w
            reach = reach[:, :, k:-k, k:-k]
        else:
            reach = b[:, PLANE["visible"]:PLANE["visible"] + 1]
        explore = (1.0 - reach) * torch.clamp(explore + 1.0, max=255.0)

        foes = b[:, PLANE["foes"]:PLANE["foes"] + 1]
        mask = F.conv2d(wrap(foes, 1), self.mask_kernel)
        has_prev = (stay >= 128.0).float()
        count_prev = torch.floor((stay - 128.0) / 16.0) * has_prev
        mask_prev = (stay - 128.0 - 16.0 * count_prev) * has_prev
        same = has_prev * torch.eq(mask, mask_prev).float()
        count = foes * same * torch.clamp(count_prev + 1.0, max=float(XATHIS_STAY_CAP))
        stay = foes * (128.0 + mask + 16.0 * count)
        return torch.cat([explore, stay], dim=1).to(torch.uint8)

    def forward(self, board: torch.Tensor, memory_in: torch.Tensor):
        f = self.trunk.features(torch.cat([board.float(), self.read(memory_in)], dim=1))
        return self.trunk.head(f), self.write(f, board, memory_in)


class PerAnt(nn.Module):
    """A memory per ant beside the board's, or alone: `planes.ANT_MEMORY` values an ant keeps for
    life, learned as `LearnedMemory`'s planes are and carried in the open under `ant_memory`.

    The inputs are the adapter's three (`planes.py`, *a memory per ant*): `ant_planes`, each ant's
    remembered values written at its own cell as `u8`; `ids`, this turn's ids modulo the table; and
    `cells`, each ant's cell in row-major order. The trunk sees the board, the board memory if there
    is one, and the ant planes read as `u8 / 127.5 - 1`; a 1x1 gate and candidate over the features
    give every cell a new value, `(1 - z) a + z tanh(c)`, which is meaningful only where an ant
    stands. The output gathers those cells (`GatherElements`, allowlisted) into `[1, N, K]`, rounds
    them back to `u8` with `Floor` and `Cast`, and writes each ant's id in front: one row an ant,
    which the runner hands back and the adapter re-keys by id next turn, so an ant's values follow
    it through every move and a new ant reads zeros.

    `inner` is `WithMemory`, `LearnedMemory` or None; with one, the graph has both memories and
    the manifest declares both outputs, priced together.
    """

    ant_ports = True

    def __init__(self, trunk: nn.Module, inner: nn.Module | None = None, planes: int = ANT_MEMORY,
                 kind: str = "learned"):
        super().__init__()
        self.trunk = trunk
        self.inner = inner
        self.channels = trunk.channels
        self.kind = kind
        self.planes = MISSION if kind == "mission" else planes
        if kind == "mission":
            # A mission head: has it one (a logit), and the wrapped offset to its target in
            # cells, scaled so tanh covers a board.
            self.mission = nn.Conv2d(trunk.channels + MISSION, MISSION, 1)
            self.aux: dict | None = None
        else:
            self.gate = nn.Conv2d(trunk.channels + planes, planes, 1)
            self.cand = nn.Conv2d(trunk.channels + planes, planes, 1)

    ant_kind = property(lambda self: self.kind)

    @property
    def memory_ports(self) -> bool:
        return self.inner is not None

    @property
    def memory_kind(self) -> str | None:
        return getattr(self.inner, "memory_kind", None)

    def forward(self, board: torch.Tensor, *rest: torch.Tensor):
        if self.inner is not None:
            memory_in, ant_planes, ids, cells = rest
        else:
            memory_in, (ant_planes, ids, cells) = None, rest
        if self.kind == "mission":
            # has as is, the offsets back to cells over 64
            ap = ant_planes.float()
            a = torch.cat([ap[:, 0:1], (ap[:, 1:3] - 128.0) / 64.0], dim=1)
        else:
            a = ant_planes.float() / 127.5 - 1.0
        parts = [board.float()]
        if self.inner is not None:
            parts.append(self.inner.read(memory_in))
        parts.append(a)
        f = self.trunk.features(torch.cat(parts, dim=1))
        outputs = [self.trunk.head(f)]
        if self.inner is not None:
            outputs.append(self.inner.write(f, board, memory_in))

        if self.kind == "mission":
            return tuple(outputs + [self._mission_rows(f, a, board, ids, cells)])

        fa = torch.cat([f, a], dim=1)
        z = torch.sigmoid(self.gate(fa))
        c = torch.tanh(self.cand(fa))
        scaled = ((1 - z) * a + z * c + 1.0) * 127.5          # [B, K, H, W] in [0, 255]
        rounded = torch.floor(scaled + 0.5)
        value = scaled + (rounded - scaled).detach() if self.training else rounded
        flat = value.flatten(2)                                 # [B, K, H*W]
        index = cells.unsqueeze(1).expand(-1, self.planes, -1)  # [B, K, N]
        rows = torch.gather(flat, 2, index).transpose(1, 2)     # [B, N, K]
        head = ids.unsqueeze(-1).to(rows.dtype)                 # [B, N, 1]
        ant_memory = torch.cat([head, rows], dim=2)
        outputs.append(ant_memory if self.training else ant_memory.to(torch.uint8))
        return tuple(outputs)

    def _mission_rows(self, f, a, board, ids, cells):
        """One row an ant, `[id, has, target_row, target_col]`: the head's has-logit rounded, and
        its offsets added to the ant's own cell, wrapping. In training the raw heads at the ants'
        cells are kept on `self.aux` for the mission loss."""
        raw = self.mission(torch.cat([f, a], dim=1))                    # [B, 3, H, W]
        flat = raw.flatten(2)                                            # [B, 3, H*W]
        index = cells.unsqueeze(1).expand(-1, MISSION, -1)               # [B, 3, N]
        at = torch.gather(flat, 2, index)                                # [B, 3, N]
        has_logit, dr, dc = at[:, 0], torch.tanh(at[:, 1]) * 64.0, torch.tanh(at[:, 2]) * 64.0
        if self.training:
            self.aux = {"has": has_logit, "dr": dr, "dc": dc}
        # The board's height and width as the graph reads them at run time (a traced size, never
        # a Python number, or the export would freeze the probe's 128).
        h = torch.as_tensor(board.shape[2]).to(torch.float32)
        w = torch.as_tensor(board.shape[3]).to(torch.float32)
        r = torch.floor(cells.float() / w)
        c = cells.float() - r * w
        tr = r + torch.floor(dr + 0.5)
        tc = c + torch.floor(dc + 0.5)
        tr = tr - h * torch.floor(tr / h)
        tc = tc - w * torch.floor(tc / w)
        has = torch.floor(torch.sigmoid(has_logit) + 0.5)
        rows = torch.stack([ids.float(), has, tr, tc], dim=2)          # [B, N, 4]
        return rows if self.training else rows.to(torch.uint8)


ARCHS = {"trunk": Trunk, "encdec": EncDec, "percell": PerCell}

MEMORIES = {"max": WithMemory, "learned": LearnedMemory, "xathis": XathisMemory,
            "xathis-sight": lambda trunk: XathisMemory(trunk, reach="sight")}
MEMORY_PLANES = {"max": N_MEMORY, "learned": N_MEMORY, "xathis": XathisMemory.planes(),
                 "xathis-sight": XathisMemory.planes()}
ANT_PLANES = {"learned": ANT_MEMORY, "mission": MISSION}


def build(spec: dict, memory: bool | str = False, ants: bool | str = False) -> nn.Module:
    """One class's entry from `classes.toml` to a module; with `memory`, one that carries a memory
    beside the board: `"max"` (or True) is `planes.MEMORY` kept with `Max`, `"learned"` is
    `LearnedMemory`, `"xathis"` the 2011 winner's two planes (`"xathis-sight"` with the view as
    its reach); with `ants`, one that carries a row per ant too: `"learned"` (or True) values the
    graph learns, `"mission"` the bot's mission."""
    arch = spec["arch"]
    if arch not in ARCHS:
        raise ValueError(f"no architecture '{arch}' (have: {', '.join(sorted(ARCHS))})")
    kwargs = {k: spec[k] for k in ("channels", "stride", "blocks", "dilate") if k in spec}
    mkind = ("max" if memory is True else memory) if memory else None
    akind = ("learned" if ants is True else ants) if ants else None
    if mkind and mkind not in MEMORIES:
        raise ValueError(f"no memory '{mkind}' (have: {', '.join(sorted(MEMORIES))})")
    if akind and akind not in ANT_PLANES:
        raise ValueError(f"no memory per ant '{akind}' (have: {', '.join(sorted(ANT_PLANES))})")
    if mkind or akind:
        kwargs["planes"] = N_PLANES + (MEMORY_PLANES[mkind] if mkind else 0) + (ANT_PLANES[akind] if akind else 0)
    net = ARCHS[arch](**kwargs)
    inner = MEMORIES[mkind](net) if mkind else None
    if akind:
        return PerAnt(net, inner, kind=akind)
    return inner or net


class ActorCritic(nn.Module):
    """What the optimiser sees. `policy` is what ships; `value` is thrown away at export.

    The critic reads the same features as the policy plus, optionally, whatever privileged numbers
    the env reports — which is standard for an asymmetric actor-critic and is safe here for a
    reason worth stating: nothing on this path survives `export.py`, so no privileged number can
    reach a model that plays.
    """

    def __init__(self, trunk: nn.Module, privileged: int = 0):
        super().__init__()
        self.trunk = trunk
        self.privileged = privileged
        self.value = nn.Sequential(
            nn.Linear(trunk.channels + privileged, 64), nn.ReLU(), nn.Linear(64, 1)
        )

    def forward(self, board: torch.Tensor, extra: torch.Tensor | None = None):
        f = self.trunk.features(board.float())
        logits = self.trunk.head(f)
        # One vector a seat. The board mean is the cheapest pooling that is size-independent, and a
        # value head has no use for spatial detail.
        pooled = f.mean(dim=(2, 3))
        if self.privileged:
            assert extra is not None, "this critic was built to take privileged input"
            pooled = torch.cat([pooled, extra], dim=1)
        return logits, self.value(pooled).squeeze(-1)


def policy_params(model: nn.Module) -> int:
    """What the class is measured on: the exported half only. A memory wrapper ships whole (its
    gates are in the graph); an actor-critic ships its trunk and leaves the value head behind."""
    exported = model if getattr(model, "memory_ports", False) else getattr(model, "trunk", model)
    return sum(p.numel() for p in exported.parameters())

"""The architectures, one per weight class.

Three things shape every net here, and none of them is "what usually works on images":

**The board wraps.** Ants maps are toroidal, so a zero-padded convolution tells the model there is a
wall at the edge where there is in fact the other side of the map. Every convolution here is padded
by wrapping, built from `cat` of edge slices — which exports to `Slice` + `Concat`, both on the
operator allowlist, where `padding_mode="circular"` exports to a `Pad` mode that only exists from
opset 18.

**The board has three sizes.** 64x96, 96x96 and 128x128, one per preset. Fully convolutional handles
that for free, and it is the main reason to stay fully convolutional; where a net downsamples, the
factor divides all three (they are all multiples of 32).

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

from .planes import N_MOVES, N_PLANES


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
        assert stride in (2, 4), "a stride that does not divide 64, 96 and 128 breaks a preset"
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


ARCHS = {"trunk": Trunk, "encdec": EncDec, "percell": PerCell}


def build(spec: dict) -> nn.Module:
    """One class's entry from `classes.toml` to a module."""
    arch = spec["arch"]
    if arch not in ARCHS:
        raise ValueError(f"no architecture '{arch}' (have: {', '.join(sorted(ARCHS))})")
    kwargs = {k: spec[k] for k in ("channels", "stride", "blocks", "dilate") if k in spec}
    return ARCHS[arch](**kwargs)


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
    """What the class is measured on: the exported half only."""
    trunk = model.trunk if isinstance(model, ActorCritic) else model
    return sum(p.numel() for p in trunk.parameters())

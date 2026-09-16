"""Behaviour cloning: fit a class's network to the teacher's moves.

    python -m tb_baselines.train.bc --class micro --data data/teacher.jsonl.gz --epochs 6

This is the **class ladder**: one fixed teacher, distilled into five byte budgets, so the question
the competition actually asks — how much play fits in N bytes — gets a measured answer rather than
an argument. It is also the reliable half of the matrix. Self-play RL on Ants is sparse-reward,
long-horizon, multi-agent and fogged; imitation is none of those, and a ladder needs opponents
before it needs interesting ones.

## The loss is per ant, not per board

The network answers a dense `[batch, 5, rows, cols]` policy map and only the cells holding one of
this seat's ants mean anything. So the loss gathers those cells and takes cross-entropy against the
teacher's move there. Everywhere else is unconstrained, and deliberately: a board is 16,384 cells
and about 30 of them have an ant on, so training the empty cells toward anything would drown the
signal five hundred to one.

One consequence is worth knowing before reading a loss curve: **a batch is a variable number of
decisions**, because seats have different ant counts. The reported loss is per ant.

## What the labels look like, measured

Over 1.16 million decisions the teacher's moves are **W 25.6% / E 25.3% / S 24.3% / N 23.8% / hold
1.0%**. There is no class imbalance to correct: an ant almost always has somewhere to be, and the
four directions are near-uniform.

That near-uniformity is also why the first dataset was thrown away. It came from a teacher that
shuffled the directions before choosing, so two equidistant neighbours — the common case in open
ground — were a coin flip, and a quarter of the labels were not a function of the state at all.
Cross-entropy against a coin flip has a floor no capacity gets past, so the class ladder would have
been measuring the teacher's noise rather than each class's capacity. The teacher is deterministic
now; see `teacher.py`.

Accuracy is the quantity being minimised and not the quantity that matters. A network can agree with
the teacher four times in five and play far worse, because the fifth compounds over three hundred
turns. Play it.
"""

from __future__ import annotations

import argparse
import gzip
import json
import random
import re
import time
from pathlib import Path

import numpy as np
import torch
import torch.nn.functional as F

from .. import nets
from ..export import budget, classes
from ..planes import MOVES, encode

MOVE_INDEX = {m: i for i, m in enumerate(MOVES)}


def device() -> torch.device:
    """MPS on Apple silicon, which is what this repository is developed on."""
    if torch.backends.mps.is_available():
        return torch.device("mps")
    if torch.cuda.is_available():
        return torch.device("cuda")
    return torch.device("cpu")


SIZE_RE = re.compile(rb'"size":\[(\d+),(\d+)\]')


class Rows:
    """The dataset as raw lines, parsed a batch at a time.

    Held as bytes rather than as parsed dicts, which is not a micro-optimisation: 250,000
    observations is about 150 MB of lines and roughly a gigabyte as Python objects, on a machine
    with sixteen. The board size is pulled out with a regex at load so batches can be grouped
    without parsing anything, and a batch of 32 costs 32 `json.loads` — nothing against the
    convolution that follows it.
    """

    def __init__(self, path: Path):
        self.lines: list[bytes] = []
        self.sizes: list[tuple[int, int]] = []
        with gzip.open(path, "rb") as f:
            self.header = json.loads(f.readline())
            for line in f:
                m = SIZE_RE.search(line)
                if not m:
                    continue
                self.lines.append(line)
                self.sizes.append((int(m.group(1)), int(m.group(2))))

    def __len__(self) -> int:
        return len(self.lines)

    def parse(self, i: int) -> dict:
        return json.loads(self.lines[i])


def batches(rows: Rows, indices: list[int], size: int, rng: random.Random, shuffle: bool = True):
    """Batches of one board size.

    Grouping by size is not a convenience: the three presets are three board sizes and one tensor
    cannot hold two. Shuffling happens inside each group, so a batch is always uniform and the
    epoch still sees the groups interleaved.
    """
    by_size: dict[tuple[int, int], list[int]] = {}
    for i in indices:
        by_size.setdefault(rows.sizes[i], []).append(i)

    chunks = []
    for group in by_size.values():
        if shuffle:
            rng.shuffle(group)
        chunks += [group[i : i + size] for i in range(0, len(group), size)]
    if shuffle:
        rng.shuffle(chunks)
    return chunks


def tensors(rows: Rows, batch: list[int], dev: torch.device):
    """A batch to `(boards, sample, row, col, label)`, flat over every ant in it."""
    parsed = [rows.parse(i) for i in batch]
    boards = np.concatenate([encode(r["o"]) for r in parsed], axis=0)
    b_idx, rr, cc, yy = [], [], [], []
    for i, r in enumerate(parsed):
        for (row, col), ch in zip(r["o"]["mine"], r["a"]):
            b_idx.append(i)
            rr.append(row)
            cc.append(col)
            yy.append(MOVE_INDEX.get(ch, MOVE_INDEX["-"]))
    return (
        torch.from_numpy(boards).to(dev),
        torch.tensor(b_idx, dtype=torch.long, device=dev),
        torch.tensor(rr, dtype=torch.long, device=dev),
        torch.tensor(cc, dtype=torch.long, device=dev),
        torch.tensor(yy, dtype=torch.long, device=dev),
    )


def ant_logits(policy: torch.Tensor, b, r, c) -> torch.Tensor:
    """`[batch, moves, rows, cols]` down to `[ants, moves]` at the cells that hold an ant.

    The same projection the adapter's `out` program does with `tb.gather`, which is the point: what
    is trained here is exactly the quantity that decides a move at play.
    """
    return policy[b, :, r, c]


def run_epoch(model, rows, chunks, dev, opt=None) -> tuple[float, float, int]:
    train = opt is not None
    model.train(train)
    total_loss = correct = seen = 0
    for batch in chunks:
        boards, b, r, c, y = tensors(rows, batch, dev)
        if y.numel() == 0:
            continue
        with torch.set_grad_enabled(train):
            logits = ant_logits(model(boards), b, r, c)
            loss = F.cross_entropy(logits, y)
            if train:
                opt.zero_grad(set_to_none=True)
                loss.backward()
                # Ant counts swing from 1 to 180, so a batch's gradient scale swings with it.
                torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
                opt.step()
        n = y.numel()
        total_loss += loss.detach().item() * n
        correct += int((logits.argmax(1) == y).sum())
        seen += n
    return total_loss / max(seen, 1), correct / max(seen, 1), seen


def override(spec: dict, args) -> dict:
    """The class's entry, with anything named on the command line replacing it."""
    spec = dict(spec)
    for key in ("arch", "channels", "blocks"):
        value = getattr(args, key, None)
        if value is not None:
            spec[key] = value
    # `percell` and `trunk` take no stride; leaving one behind would be passed to a constructor that
    # does not accept it, at the end of a long training run rather than the start.
    if spec.get("arch") in ("trunk", "percell"):
        spec.pop("stride", None)
    return spec


def main(argv: list[str] | None = None) -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--class", dest="cls", required=True)
    # A variant is a command line, not a second table: the run's history.json and the model card
    # both record what was actually built, which is the copy that matters.
    ap.add_argument("--arch", default=None, help="override the class's architecture")
    ap.add_argument("--channels", type=int, default=None)
    ap.add_argument("--blocks", type=int, default=None)
    ap.add_argument("--data", type=Path, default=Path("data/teacher.jsonl.gz"))
    ap.add_argument("--epochs", type=int, default=6)
    ap.add_argument("--batch", type=int, default=32)
    ap.add_argument("--lr", type=float, default=2e-3)
    ap.add_argument("--holdout", type=float, default=0.05)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--out", type=Path, default=None)
    a = ap.parse_args(argv)

    torch.manual_seed(a.seed)
    rng = random.Random(a.seed)
    dev = device()

    rows = Rows(a.data)
    header = rows.header
    order = list(range(len(rows)))
    rng.shuffle(order)
    cut = int(len(order) * (1 - a.holdout))
    train_idx, val_idx = order[:cut], order[cut:]

    spec = override(classes()[a.cls], a)
    model = nets.build(spec).to(dev)
    b = budget(a.cls)
    print(f"{a.cls}: {nets.policy_params(model):,} parameters "
          f"(budget about {b['params_at_target']:,}), {dev.type}")
    print(f"data: {len(train_idx):,} train / {len(val_idx):,} held out, "
          f"engine {header['engine_digest'][:20]}")

    opt = torch.optim.AdamW(model.parameters(), lr=a.lr, weight_decay=1e-4)
    sched = torch.optim.lr_scheduler.CosineAnnealingLR(opt, T_max=max(a.epochs, 1))
    val_chunks = batches(rows, val_idx, a.batch, rng, shuffle=False)

    out = a.out or Path("runs") / f"{a.cls}-bc"
    out.mkdir(parents=True, exist_ok=True)
    best = float("inf")
    history = []
    for epoch in range(1, a.epochs + 1):
        t0 = time.time()
        chunks = batches(rows, train_idx, a.batch, rng)
        tl, ta, n = run_epoch(model, rows, chunks, dev, opt)
        vl, va, _ = run_epoch(model, rows, val_chunks, dev)
        sched.step()
        took = time.time() - t0
        print(f"  epoch {epoch}/{a.epochs}  train {tl:.4f} / {ta:.1%}   "
              f"held out {vl:.4f} / {va:.1%}   {n:,} ants  {took:.0f}s")
        history.append({"epoch": epoch, "train_loss": tl, "train_acc": ta,
                        "val_loss": vl, "val_acc": va, "seconds": round(took, 1)})
        if vl < best:
            best = vl
            torch.save({"trunk": model.state_dict(), "class": a.cls,
                        "engine_digest": header["engine_digest"]}, out / "best.pt")

    (out / "history.json").write_text(json.dumps(
        {"class": a.cls, "method": "bc", "arch": spec["arch"], "spec": spec,
         "data": str(a.data), "epochs": history,
         "engine_digest": header["engine_digest"], "device": dev.type}, indent=2) + "\n")
    print(f"  -> {out / 'best.pt'}  (best held-out loss {best:.4f})")
    print("  accuracy against the teacher is not strength. Play it: `python -m tb_baselines.eval`")


if __name__ == "__main__":
    main()

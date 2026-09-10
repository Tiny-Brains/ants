#!/usr/bin/env python3
"""Conformance check for the TinyBrains protocol.

Three layers:
  1. The cartridge registration document -- the platform's only schema.
  2. The Ants state/action schemas -- authored by the game, published for model
     developers, and NEVER loaded by the platform. Checked here because the game
     developer is the platform owner and a broken example helps nobody.
  3. The invariants no JSON Schema can express: action length must match the
     ant list, run-lengths must cover the map, and payloads must be
     observer-relative and canonically serializable.
"""
import json, os, sys, warnings
warnings.filterwarnings("ignore")
from jsonschema import Draft202012Validator as V

HERE = os.path.dirname(os.path.abspath(__file__))
jload = lambda *p: json.load(open(os.path.join(HERE, *p)))
CV = V(jload("tb-cartridge.schema.json"))
MAX_PAYLOAD_BYTES = 1024 * 1024          # platform constant, not per-game

fails = 0
def check(cond, msg):
    global fails
    if not cond:
        fails += 1; print("FAIL ", msg)

# ---- 1. cartridge registration ----
CARTRIDGES = [("ants", jload("..", "cartridge.json"))] + \
             [(g, jload("other-games", f"{g}.json")["cartridge"]) for g in ("tron", "planetwars")]
for name, c in CARTRIDGES:
    check(CV.is_valid(c), f"{name}.cartridge should validate")
print(f"cartridge positive: {len(CARTRIDGES)} documents checked")

_ants = lambda: jload("..", "cartridge.json")
def mut(fn):
    d = _ants(); fn(d); return d

NEG = [
    ("missing budgets",         mut(lambda d: d.pop("budgets"))),
    ("missing presets",         mut(lambda d: d.pop("presets"))),
    ("missing abi",             mut(lambda d: d.pop("abi"))),
    ("unknown abi",             mut(lambda d: d.update(abi=2))),
    ("one-player preset",       mut(lambda d: d.update(presets=[{"name": "solo", "players": 1}]))),
    ("empty preset list",       mut(lambda d: d.update(presets=[]))),
    ("preset with capitals",    mut(lambda d: d.update(presets=[{"name": "Standard", "players": 2}]))),
    ("preset as a bare string", mut(lambda d: d.update(presets=["standard"]))),
    ("preset without seats",    mut(lambda d: d.update(presets=[{"name": "standard"}]))),
    ("top-level players",       mut(lambda d: d.update(players=[2]))),
    ("bad version string",      mut(lambda d: d.update(version="1.0"))),
    ("bad game id",             mut(lambda d: d.update(game="Ants!"))),
    ("incomplete flop caps",    mut(lambda d: d["budgets"].update(flop_caps={"nano": 1e8}))),
    ("zero flop cap",           mut(lambda d: d["budgets"]["flop_caps"].update(micro=0))),
    ("max_turns of zero",       mut(lambda d: d["limits"].update(max_turns=0))),
    ("missing turn_ms",         mut(lambda d: d["limits"].pop("turn_ms"))),
    ("leftover protocol field", mut(lambda d: d.update(protocol=1))),
    ("leftover visualizer",     mut(lambda d: d.update(visualizer={"module": "viz.js"}))),
    ("leftover docs pointer",   mut(lambda d: d.update(docs={"url": "https://x"}))),
    ("per-game message cap",    mut(lambda d: d["limits"].update(max_message_kib=512))),
    ("about without links",     mut(lambda d: d["about"].pop("links"))),
    ("about link not https",    mut(lambda d: d["about"]["links"].append({"label": "x", "href": "http://x"}))),
    ("about with markup shape", mut(lambda d: d["about"].update(html="<b>x</b>"))),
]
for name, doc in NEG:
    check(not CV.is_valid(doc), f"cartridge should have been rejected: {name}")
print(f"cartridge negative: {len(NEG)} documents checked")

# ---- 2. the Ants game schemas ----
SV = V(jload("state.schema.json"))
AV = V(jload("action.schema.json"))
CASES = ["opening", "midgame", "wiped"]
for c in CASES:
    ex = jload("examples", f"{c}.json")
    check(SV.is_valid(ex["state"]),  f"ants/{c}.state should validate")
    check(AV.is_valid(ex["action"]), f"ants/{c}.action should validate")
print(f"ants schemas: {len(CASES)} state/action pairs checked")

base = lambda: jload("examples", "midgame.json")
def smut(fn):
    d = base()["state"]; fn(d); return d

SNEG = [
    ("extra field",             smut(lambda d: d.update(turn=42))),
    ("scores left in",          smut(lambda d: d.update(scores=[4, 2]))),
    ("visibility mask left in", smut(lambda d: d.update(vis="AAAA"))),
    ("missing mine",            smut(lambda d: d.pop("mine"))),
    ("missing food",            smut(lambda d: d.pop("food"))),
    ("my ant carrying owner",   smut(lambda d: d["mine"].append([1, 2, 0]))),
    ("foe without owner",       smut(lambda d: d["foes"].append([1, 2]))),
    ("foe owned by me",         smut(lambda d: d["foes"].append([1, 2, 0]))),
    ("negative coordinate",     smut(lambda d: d["mine"].append([-1, 3]))),
    ("3-element size",          smut(lambda d: d.update(size=[64, 96, 2]))),
    ("water as bare array",     smut(lambda d: d.update(water=[0, 6144]))),
    ("water with extra key",    smut(lambda d: d["water"].update(dense=[]))),
]
for name, doc in SNEG:
    check(not SV.is_valid(doc), f"ants state should have been rejected: {name}")

ANEG = [("unknown direction", ["N", "X"]), ("lowercase", ["n"]),
        ("nested", [["N"]]), ("object not array", {"moves": ["N"]}), ("null move", ["N", None])]
for name, doc in ANEG:
    check(not AV.is_valid(doc), f"ants action should have been rejected: {name}")
print(f"ants negative: {len(SNEG) + len(ANEG)} documents checked")

# ---- 3. cross-field invariants no schema can express ----
for c in CASES:
    ex = jload("examples", f"{c}.json")
    st, act = ex["state"], ex["action"]
    rows, cols = st["size"]
    runs = st["water"]["rle"]
    check(len(act) == len(st["mine"]),
          f"ants/{c}: action length {len(act)} must equal len(mine) {len(st['mine'])}")
    check(len(runs) % 2 == 0, f"ants/{c}: run-length list must be pairs")
    check(sum(runs[1::2]) == rows * cols,
          f"ants/{c}: runs sum to {sum(runs[1::2])}, map has {rows * cols} cells")
    check(all(v in (0, 1) for v in runs[0::2]), f"ants/{c}: run values must be 0 or 1")
print(f"cross-field: {len(CASES)} x 4 invariants checked")

# ---- payload rules (root may be ANY JSON value: Ants actions are arrays, Tron's is a string) ----
canon = lambda o: json.dumps(o, sort_keys=True, separators=(",", ":"))
payloads = []
for c in CASES:
    ex = jload("examples", f"{c}.json"); payloads += [ex["state"], ex["action"]]
for g in ("tron", "planetwars"):
    ex = jload("other-games", f"{g}.json"); payloads += [ex["state"], ex["action"]]
for i, p in enumerate(payloads):
    check(len(canon(p).encode()) <= MAX_PAYLOAD_BYTES, f"payload {i} exceeds the cap")
    check(json.loads(canon(p)) == p, f"payload {i} must survive canonical round-trip")
print(f"payload rules: {len(payloads)} payloads x 2 rules checked")

# Observer-relative: relabel every seat, ask the same player under its new label,
# and the bytes must be identical. The real suite runs this against the cartridge
# over many seeded matches; the pure function below stands in.
NP = 2
BOARD = [(12, 30, 0), (12, 33, 1), (20, 20, 1)]
permute = lambda b, p: [(r, c, p[s]) for r, c, s in b]
def view(b, me):
    return {"mine": sorted([r, c] for r, c, s in b if s == me),
            "foes": sorted([r, c, (s - me) % NP] for r, c, s in b if s != me)}
leaky = lambda b, me: {**view(b, me), "seat": me}
PERM = {0: 1, 1: 0}
for player in range(NP):
    check(canon(view(BOARD, player)) == canon(view(permute(BOARD, PERM), PERM[player])),
          f"observer-relative: player {player} must see identical bytes after a seat relabel")
check(canon(leaky(BOARD, 0)) != canon(leaky(permute(BOARD, PERM), PERM[0])),
      "the property must actually catch a payload that leaks its absolute seat")
print("observer-relative: 3 properties checked")

print("\nALL CHECKS PASSED" if not fails else f"\n{fails} FAILURE(S)")
sys.exit(1 if fails else 0)

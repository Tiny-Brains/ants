"""xathis's AI Challenge 2011 bot, the contest's winner, as a teacher: a port of `Strategy.java`.

    https://github.com/xathis/AI-Challenge-2011-bot   (Strategy.java, 1,773 lines)

**It is a port, not a rewrite.** The methods, their order (`actions()`), their constants, their
tie-breaks and their bugs are the Java's, so that what a model distils is the bot that won,
and not this repository's idea of it. Where the Java left something to its runtime, this file
names the choice:

- **Wall-clock timeouts are evaluation budgets.** The Java cuts the combat search at 420 ms of the
  turn and the missions at 470 ms. A teacher has to be a function of its input, so the search
  counts leaf evaluations instead and cuts at `SEARCH_BUDGET`; missions never time out and are
  updated every turn, which is what the Java does whenever the turn is young (`time < 250`).
- **`turnRandom` is `java.util.Random(turn)`**, ported bit for bit (`JavaRandom`), so a random
  border for a hill ant is the border the Java would have picked.
- **Two comparators add a random term inside `compare`** (`defComp`, findPrecGroup's `comp`).
  Java's `TreeSet` then orders by the walk it happened to take; here the elements are sorted by
  the key with one random draw each as the tie-break, which is the intent and reproducible.
- **Iteration order is the observation's.** The Java's ant list is the order the engine printed
  them; here it is `mine` (row-major, the engine's order) then `foes` as the view lists them.
- **Water shrinks a tile's neighbours in the Java** (`removeNeighbor`); here a tile's neighbours
  are the passable ones in the Java's parity order (`n s e w` on odd tiles, `e w n s` on even),
  recomputed when known water grows, which is the same list.

The bot keeps three things between turns, and they are what a model built on it has to carry
(`planes.XATHIS_MEMORY`, `XathisState`): each tile's `exploreValue`, its age since an own ant was
within ten steps (100 at the start, so an unseen tile outranks any explored one); each tile's
`stayValue`/`stayTurnCount`, the enemy-stillness detector (`willStay` after five turns of the same
neighbours); and each ant's mission, a border tile it walks to. Water it learns from the view,
which already carries known water. Enemy hills and food it does not remember: `Connection.update`
rebuilds both from the visible turn, so neither does this.
"""

from __future__ import annotations

from typing import Optional

import numpy as np

from .planes import Board

# ---- the Java runtime ---------------------------------------------------------------------

MASK48 = (1 << 48) - 1


def _i32(x: int) -> int:
    x &= 0xFFFFFFFF
    return x - (1 << 32) if x & 0x80000000 else x


class JavaRandom:
    """`java.util.Random`, the two calls the bot makes: `nextInt(bound)` and, through
    `compare`, `nextInt(100)`."""

    def __init__(self, seed: int):
        self.seed = (seed ^ 0x5DEECE66D) & MASK48

    def next(self, bits: int) -> int:
        self.seed = (self.seed * 0x5DEECE66D + 0xB) & MASK48
        return _i32(self.seed >> (48 - bits))

    def next_int(self, bound: int) -> int:
        if bound <= 0:
            raise ValueError("bound must be positive")
        if bound & (bound - 1) == 0:
            return _i32((bound * self.next(31)) >> 31)
        while True:
            bits = self.next(31)
            val = bits % bound
            if _i32(bits - val + (bound - 1)) >= 0:
                return val


# ---- the map ------------------------------------------------------------------------------

UNSEEN, WATER, FOOD, LAND, MY_ANT = -4, -3, -2, -1, 0
# An enemy is any type above MY_ANT: the observation's owner (1 and up) is the Java's PLAYERk.

CLOSE_ENEMY_RADIUS = 9
CLOSE_ENEMY_RADIUS2 = CLOSE_ENEMY_RADIUS ** 2
AREA_DIST = 20
SEARCH_BUDGET = 20_000      # leaf evaluations a combat search may make before it is cut

DIRS = "NESW"


class Tile:
    __slots__ = ("row", "col", "type", "ant", "old_ant", "neighbors", "is_hill", "hill_player",
                 "dist", "hill_dist", "prev", "is_reached", "source", "explore_value",
                 "prev_firsts", "f", "is_reached_by_me", "has_virt_ant", "start_tile",
                 "is_in_my_area", "is_border", "stay_turn_count", "stay_value", "is_checked")

    def __init__(self, row: int, col: int):
        self.row, self.col = row, col
        self.type = LAND
        self.ant: Optional[Ant] = None
        self.old_ant: Optional[Ant] = None
        self.neighbors: list[Tile] = []
        self.is_hill = False
        self.hill_player = None
        self.dist = 0
        self.hill_dist = 1 << 30
        self.prev: Optional[Tile] = None
        self.is_reached = False
        self.source: Optional[Tile] = None
        self.explore_value = 100
        self.prev_firsts: set = set()
        self.f = 0
        self.is_reached_by_me = False
        self.has_virt_ant = False
        self.start_tile: Optional[Tile] = None
        self.is_in_my_area = False
        self.is_border = False
        self.stay_turn_count = 0
        self.stay_value = -1
        self.is_checked = False

    def is_free(self) -> bool:
        return self.type == LAND

    def is_enemy(self) -> bool:
        return self.type > MY_ANT

    def dir_to(self, to: "Tile", rows: int, cols: int) -> str:
        """`Tile.dirTo`: the direction to a direct neighbour, wrapping."""
        if to.row == self.row:
            if to.col == self.col + 1:
                return "E"
            if to.col == self.col - 1:
                return "W"
            return "W" if self.col == 0 else "E"
        if to.row == self.row + 1:
            return "S"
        if to.row == self.row - 1:
            return "N"
        return "N" if self.row == 0 else "S"


class Ant:
    __slots__ = ("tile", "is_dangered", "is_indirectly_dangered", "has_moved", "has_mission",
                 "is_detached", "close_enemy_dists", "closest_enemy_tile", "close_enemy_dists_sum",
                 "closest_enemy", "closest_enemy_dist", "num_close_enemies", "gamma_dist_enemies",
                 "is_dead", "weakness", "is_reached", "is_grouped", "is_gamma_grouped", "curr_to",
                 "best_to", "comp_value", "mission", "num_close_own_ants", "will_stay", "dep_table",
                 "check_all", "check_neighbors", "dist_map")

    def __init__(self, tile: Tile):
        self.tile = tile
        self.is_dangered = False
        self.is_indirectly_dangered = False
        self.has_moved = False
        self.has_mission = False
        self.is_detached = True
        # A Java TreeMap<Integer, Ant>: one ant a distance, the later one winning the key.
        self.close_enemy_dists: dict[int, Ant] = {}
        self.closest_enemy_tile: Optional[Tile] = None
        self.close_enemy_dists_sum = 0
        self.closest_enemy: Optional[Ant] = None
        self.closest_enemy_dist = 1 << 30
        self.num_close_enemies = 0
        self.gamma_dist_enemies: list[Ant] = []
        self.is_dead = False
        self.weakness = 0
        self.is_reached = False
        self.is_grouped = False
        self.is_gamma_grouped = False
        self.curr_to: Optional[Tile] = None
        self.best_to: Optional[Tile] = None
        self.comp_value = 0
        self.mission: Optional[Mission] = None
        self.num_close_own_ants = 0
        self.will_stay = False
        self.dep_table: dict = {}
        self.check_all = False
        self.check_neighbors: list[Tile] = []
        self.dist_map: dict = {}


class Mission:
    __slots__ = ("target", "curr_tile", "last_updated", "is_removed")

    def __init__(self):
        self.target: Optional[Tile] = None
        self.curr_tile: Optional[Tile] = None
        self.last_updated = 0
        self.is_removed = False


class Area:
    __slots__ = ("ants", "tiles", "contains_hill", "hill", "border", "is_checked", "player")

    def __init__(self):
        self.ants: list[Ant] = []
        self.tiles: list[Tile] = []
        self.contains_hill = False
        self.hill: Optional[Tile] = None
        self.border: list[Tile] = []
        self.is_checked = False
        self.player = None


class GammaGroup:
    __slots__ = ("my_ants", "enemy_ants", "max_num_close_own_ants")

    def __init__(self):
        self.my_ants: list[Ant] = []
        self.enemy_ants: list[Ant] = []
        self.max_num_close_own_ants = 0


# ---- the bot ------------------------------------------------------------------------------

class Xathis:
    """One seat of one match: `Connection` and `Strategy` together, fed from the observation.

    `orders(obs)` is one turn. It returns a move a character an ant in `mine`'s order, and keeps
    the three things the bot remembers for the next call. A new match is a new instance.
    """

    def __init__(self):
        self.rows = self.cols = 0
        self.map: list[list[Tile]] = []
        self.turn = 0
        self.known_water: Optional[np.ndarray] = None
        self.ants: list[Ant] = []
        self.hills: list[Tile] = []
        self.foods: list[Tile] = []
        self.my_hills: list[Tile] = []
        self.enemy_hills: list[Tile] = []
        self.my_ants: list[Ant] = []
        self.enemy_ants: list[Ant] = []
        self.dangered_ants: list[Ant] = []
        self.enemy_ants_ordered: list[Ant] = []
        self.areas: list[Area] = []
        self.fight_areas: list[Area] = []
        self.missions: list[Mission] = []
        self.gamma_groups: list[GammaGroup] = []
        self.distribute_check_dist = 9
        self.is_timeout = False
        self.turn_random = JavaRandom(0)
        self.is_mission_phase = False
        self.be_aggressive = False
        self.my_ants_prec: list[Ant] = []
        self.enemy_ants_prec: list[Ant] = []
        self.best_prec_value = 0
        self.my_move_safe_count = 0
        self.enemy_safe_count = 0
        self.do_cut = False
        self.evaluations = 0
        self.orders_out: dict[tuple[int, int], str] = {}

    # ---- Connection: the map and the turn's input ------------------------------------

    def _setup(self, rows: int, cols: int) -> None:
        self.rows, self.cols = rows, cols
        self.map = [[Tile(r, c) for c in range(cols)] for r in range(rows)]
        self.known_water = np.zeros((rows, cols), dtype=bool)
        self._link(np.zeros((rows, cols), dtype=bool))

    def _link(self, water: np.ndarray, tiles=None) -> None:
        """Each tile's passable neighbours in the Java's parity order, a water tile removed;
        `tiles` limits it to the ones beside newly known water."""
        rows, cols, m = self.rows, self.cols, self.map
        every = tiles if tiles is not None else (t for row in m for t in row)
        for t in every:
            r, c = t.row, t.col
            if water[r, c]:
                t.neighbors = []
                continue
            n, s = m[(r - 1) % rows][c], m[(r + 1) % rows][c]
            e, w = m[r][(c + 1) % cols], m[r][(c - 1) % cols]
            order = (n, s, e, w) if (r + c) & 1 else (e, w, n, s)
            t.neighbors = [x for x in order if not water[x.row, x.col]]

    def _update(self, obs: dict) -> None:
        """`Connection.update`: clear last turn's ants, food and hills; read this turn's."""
        for ant in self.ants:
            ant.tile.type = LAND
            ant.tile.ant = None
        self.ants = []
        for food in self.foods:
            food.type = LAND
        self.foods = []
        for hill in self.hills:
            hill.type = LAND
            hill.is_hill = False
        self.hills = []

        rows, cols = obs["size"]
        water = Board(rows, cols).rle(obs["water"]["rle"]).astype(bool)
        new = water & ~self.known_water
        if new.any():
            self.known_water |= water
            touched = set()
            for r, c in np.argwhere(new):
                t = self.map[r][c]
                t.type = WATER
                touched.add(t)
                touched.update(t.neighbors)
                t.neighbors = []
            self._link(self.known_water, touched)

        m = self.map
        for r, c in obs["mine"]:
            t = m[r][c]
            t.type = MY_ANT
            t.ant = Ant(t)
            self.ants.append(t.ant)
        for r, c, owner in obs["foes"]:
            t = m[r][c]
            t.type = int(owner)
            t.ant = Ant(t)
            self.ants.append(t.ant)
        for r, c, owner in obs["hills"]:
            t = m[r][c]
            t.is_hill = True
            t.hill_player = int(owner)
            self.hills.append(t)
        for r, c in obs["food"]:
            t = m[r][c]
            t.type = FOOD
            self.foods.append(t)

    def issue_order(self, tile: Tile, direction: str) -> None:
        self.orders_out[(tile.row, tile.col)] = direction

    # ---- the turn ----------------------------------------------------------------------

    def orders(self, obs: dict) -> np.ndarray:
        """One turn: a move index an ant (`planes.MOVES` order), aligned with `mine`."""
        rows, cols = obs["size"]
        if not self.map:
            self._setup(rows, cols)
        self._update(obs)
        self.orders_out = {}
        self._actions()
        self.turn += 1
        out = np.full(len(obs["mine"]), 4, dtype=np.int64)
        for i, (r, c) in enumerate(obs["mine"]):
            d = self.orders_out.get((r, c))
            if d is not None:
                out[i] = DIRS.index(d)
        return out

    def _actions(self) -> None:
        self._init_turn()
        self._calc_num_close_enemies()
        self._init_missions()
        self._enemy_hills()
        self._food()
        self._init_explore()
        self._create_areas()
        self._fight()
        self._defence()
        self._approach_enemies()
        self._attack_detached_enemies()
        self._escape_enemies()
        self._distribute(True)
        self._explore()
        self._do_missions()
        self._create_missions()
        self._distribute(False)
        self._clean_areas()

    # ---- geometry ----------------------------------------------------------------------

    def dist(self, a: Tile, b: Tile) -> int:
        d_col = abs(a.col - b.col)
        d_row = abs(a.row - b.row)
        return min(d_col, self.cols - d_col) + min(d_row, self.rows - d_row)

    def dist_row(self, a: Tile, b: Tile) -> int:
        d = abs(a.row - b.row)
        return min(d, self.rows - d)

    def dist_col(self, a: Tile, b: Tile) -> int:
        d = abs(a.col - b.col)
        return min(d, self.cols - d)

    def is_alpha_dist(self, a: Tile, b: Tile) -> bool:
        dx = self.dist_row(a, b)
        dy = self.dist_col(a, b)
        return (dy <= 1 and dx <= 2) or (dy == 2 and dx <= 1)

    def is_beta_dist(self, a: Tile, b: Tile) -> bool:
        dx = self.dist_row(a, b)
        dy = self.dist_col(a, b)
        if dx + dy <= 4:
            return not ((dx == 0 and dy == 4) or (dy == 0 and dx == 4))
        return False

    def is_gamma_dist(self, a: Tile, b: Tile) -> bool:
        dx = self.dist_row(a, b)
        dy = self.dist_col(a, b)
        if dx + dy <= 5:
            return not ((dx == 0 and dy == 5) or (dy == 0 and dx == 5))
        return False

    # ---- initTurn ----------------------------------------------------------------------

    def _init_turn(self) -> None:
        self.my_ants = []
        self.enemy_ants = []
        self.dangered_ants = []
        self.my_hills = []
        self.enemy_hills = []
        self.gamma_groups = []
        self.is_timeout = False
        self.evaluations = 0
        self.turn_random = JavaRandom(self.turn)

        for ant in self.ants:
            (self.my_ants if ant.tile.type == MY_ANT else self.enemy_ants).append(ant)
        for hill in self.hills:
            (self.my_hills if hill.hill_player == MY_ANT else self.enemy_hills).append(hill)

        # my - my: the Java walks each ant against the ones after it
        my = self.my_ants
        for i, ant1 in enumerate(my):
            for j in range(len(my) - 1, i, -1):
                ant2 = my[j]
                if self.dist_row(ant1.tile, ant2.tile) <= 5 and self.dist_col(ant1.tile, ant2.tile) <= 5:
                    ant1.num_close_own_ants += 1
                    ant2.num_close_own_ants += 1

        # my - enemy
        for my_ant in my:
            my_ant.tile.old_ant = my_ant
            for enemy in self.enemy_ants:
                dy = self.dist_row(my_ant.tile, enemy.tile)
                if dy > CLOSE_ENEMY_RADIUS:
                    continue
                dx = self.dist_col(my_ant.tile, enemy.tile)
                if dx > CLOSE_ENEMY_RADIUS:
                    continue
                d = dy * dy + dx * dx
                if d <= CLOSE_ENEMY_RADIUS2:
                    my_ant.close_enemy_dists[d] = enemy
                    enemy.close_enemy_dists[d] = my_ant
                    my_ant.close_enemy_dists_sum += d
                    enemy.close_enemy_dists_sum += d
                    if dx + dy <= 5 and not ((dx == 0 and dy == 5) or (dy == 0 and dx == 5)):
                        my_ant.is_indirectly_dangered = True
                        my_ant.gamma_dist_enemies.append(enemy)
                        enemy.gamma_dist_enemies.append(my_ant)
                        if (not my_ant.is_dangered and dx + dy <= 4
                                and not ((dx == 0 and dy == 4) or (dy == 0 and dx == 4))):
                            my_ant.is_dangered = True
                            self.dangered_ants.append(my_ant)
            if my_ant.close_enemy_dists:
                my_ant.closest_enemy_tile = my_ant.close_enemy_dists[min(my_ant.close_enemy_dists)].tile

        # enemy - enemy, and the enemies ordered by how surrounded they are
        ordered: dict[int, Ant] = {}
        en = self.enemy_ants
        for i, ant1 in enumerate(en):
            ordered[-len(ant1.close_enemy_dists) * CLOSE_ENEMY_RADIUS + ant1.close_enemy_dists_sum] = ant1
            for j in range(len(en) - 1, i, -1):
                ant2 = en[j]
                if self.dist_row(ant1.tile, ant2.tile) <= 5 and self.dist_col(ant1.tile, ant2.tile) <= 5:
                    ant1.is_detached = False
                    ant2.is_detached = False
                    break
        self.enemy_ants_ordered = [ordered[k] for k in sorted(ordered)]

        self.distribute_check_dist = 9 if len(my) < 160 else 8
        for row in self.map:
            for tile in row:
                tile.explore_value += 1
                tile.is_border = False
                if tile.ant is None:
                    tile.stay_value = -1

        for enemy in self.enemy_ants:
            curr_stay_value = 0
            for i, n in enumerate(enemy.tile.neighbors):
                curr_stay_value |= (1 if n.is_enemy() else 0) << i
            if enemy.tile.stay_value == curr_stay_value:
                enemy.tile.stay_turn_count += 1
                if enemy.tile.stay_turn_count >= 5:
                    enemy.will_stay = True
            else:
                enemy.tile.stay_value = curr_stay_value
                enemy.tile.stay_turn_count = 0
                enemy.will_stay = False

    def _calc_num_close_enemies(self) -> None:
        for enemy in self.enemy_ants:
            if not enemy.close_enemy_dists:
                continue
            open_list = [enemy.tile]
            changed = [enemy.tile]
            enemy.tile.dist = 0
            enemy.tile.is_reached = True
            head = 0
            while head < len(open_list):
                tile = open_list[head]
                head += 1
                if tile.dist >= 12:
                    break
                for n in tile.neighbors:
                    if n.is_reached:
                        continue
                    n.is_reached = True
                    n.dist = tile.dist + 1
                    changed.append(n)
                    open_list.append(n)
                    if n.type == MY_ANT:
                        n.ant.num_close_enemies += 1
                        if n.ant.closest_enemy_dist > n.dist:
                            n.ant.closest_enemy = enemy
                            n.ant.closest_enemy_dist = n.dist
            for t in changed:
                t.is_reached = False

    # ---- missions ----------------------------------------------------------------------

    def _init_missions(self) -> None:
        kept = []
        for m in self.missions:
            if m.is_removed:
                continue
            ant = m.curr_tile.ant
            if ant is None or ant.tile.type != MY_ANT:
                continue
            ant.has_mission = True
            ant.mission = m
            kept.append(m)
        self.missions = kept

    def _do_missions(self) -> None:
        self.is_timeout = False
        self.is_mission_phase = True
        for m in list(self.missions):
            self._do_mission(m)
        self.is_mission_phase = False

    def _do_mission(self, m: Mission) -> None:
        if m.is_removed:
            return
        ant = m.curr_tile.ant
        if ant is None or ant.has_moved:
            if ant is not None:
                ant.has_moved = True
            return
        # The Java updates when the mission is ten turns old or the turn is young; the turn is
        # always young here.
        self._update_mission(m)
        dest = self._a_star2(ant.tile, m.target, True)
        if dest is None:
            m.is_removed = True
            ant.has_mission = False
            return
        ant.has_mission = True
        self._do_move(ant.tile, dest)
        if dest is m.target:
            m.is_removed = True
        else:
            m.curr_tile = dest

    def _update_mission(self, m: Mission) -> None:
        start = m.curr_tile if m.target.type == WATER else m.target
        border = self._find_border(start)
        m.last_updated = self.turn
        if border is not None:
            m.target = border

    def _create_missions(self) -> None:
        if self.is_timeout:
            return
        for area in self.fight_areas:
            if len(area.ants) < 2 or not area.border:
                continue
            for ant in area.ants:
                if ant.has_mission or ant.has_moved:
                    continue
                if ant.tile.is_hill:
                    target = area.border[self.turn_random.next_int(len(area.border))]
                else:
                    target = self._find_border(ant.tile)
                if target is None:
                    continue
                dest = self._a_star2(ant.tile, target, True)
                if dest is None:
                    continue
                self._do_move(ant.tile, dest)
                ant.has_mission = True
                m = Mission()
                m.curr_tile = dest
                m.target = target
                m.last_updated = self.turn
                self.missions.append(m)

    def _find_border(self, start: Tile) -> Optional[Tile]:
        if start.is_border:
            return start
        border = None
        open_list = [start]
        changed = [start]
        start.dist = 0
        start.is_reached = True
        head = 0
        while head < len(open_list):
            tile = open_list[head]
            head += 1
            if tile.dist >= 400:
                break
            for n in tile.neighbors:
                if n.is_border:
                    border = n
                    break
                if n.is_reached:
                    continue
                n.is_reached = True
                n.dist = tile.dist + 1
                changed.append(n)
                open_list.append(n)
            if border is not None:
                break
        for t in changed:
            t.is_reached = False
        return border

    # ---- defence -----------------------------------------------------------------------

    def _defence(self) -> None:
        if len(self.my_hills) > 4:
            return
        for hill in self.my_hills:
            self._defend_hill(hill)

    def _defend_hill(self, hill: Tile) -> None:
        dangerous: list[Tile] = []
        open_list = [hill]
        changed = [hill]
        hill.hill_dist = 0
        hill.is_reached = True
        hill.prev = None
        head = 0
        while head < len(open_list):
            tile = open_list[head]
            head += 1
            if tile.hill_dist >= 14:
                break
            for n in tile.neighbors:
                if n.is_reached:
                    continue
                n.is_reached = True
                n.hill_dist = tile.hill_dist + 1
                if n.hill_dist <= 10:
                    n.explore_value = 0
                n.prev = tile
                changed.append(n)
                open_list.append(n)
                if n.is_enemy():
                    dangerous.append(n)
        for t in changed:
            t.is_reached = False
        # defComp: by hill distance, a random draw breaking ties
        dangerous.sort(key=lambda t: (t.hill_dist, self.turn_random.next_int(100)))

        for enemy_tile in dangerous:
            dist = enemy_tile.hill_dist
            half_dist = dist // 2
            quarter_dist = dist // 4
            # `while (dist-- > halfDist)`: the failing test decrements once more.
            half_tile = enemy_tile
            if half_dist > 1:
                while dist > half_dist:
                    dist -= 1
                    half_tile = half_tile.prev
                dist -= 1
            else:
                half_tile = hill
            quarter_tile = half_tile
            if quarter_dist > 1:
                while dist > quarter_dist:
                    dist -= 1
                    quarter_tile = quarter_tile.prev
                dist -= 1
            else:
                quarter_tile = hill

            t = enemy_tile
            found = False
            defender = None
            skip = False
            while not t.is_hill:
                if t.type == MY_ANT and not t.ant.has_moved:
                    found = True
                    defender = t.ant
                    break
                elif t.type == MY_ANT and t.ant.has_moved and t.ant.is_grouped and t.hill_dist > 3:
                    skip = True
                    break
                if not found:
                    for n in t.neighbors:
                        if n.type == MY_ANT and not n.ant.has_moved:
                            found = True
                            defender = n.ant
                            break
                        elif n.type == MY_ANT and n.ant.has_moved and n.ant.is_grouped and t.hill_dist > 3:
                            skip = True
                            break
                    if found or skip:
                        break
                t = t.prev
            if skip:
                continue
            if not found:
                open_list = [quarter_tile]
                changed = [quarter_tile]
                quarter_tile.dist = 0
                quarter_tile.is_reached = True
                head = 0
                while head < len(open_list):
                    tile = open_list[head]
                    head += 1
                    if tile.dist >= 30:
                        break
                    for n in tile.neighbors:
                        if n.is_reached:
                            continue
                        if n.type == MY_ANT and not n.ant.has_moved:
                            defender = n.ant
                            found = True
                            break
                        n.is_reached = True
                        n.dist = tile.dist + 1
                        changed.append(n)
                        open_list.append(n)
                    if found:
                        break
                for x in changed:
                    x.is_reached = False
            if not found:
                continue
            dest = self._a_star(defender.tile, half_tile, True)
            if dest is None or self._is_suicide(defender, dest):
                continue
            self._do_move(defender.tile, dest)

    # ---- enemy hills and food ----------------------------------------------------------

    def _enemy_hills(self) -> None:
        for hill_tile in self.enemy_hills:
            count = 1 if len(self.my_ants) <= 10 else 4
            changed = [hill_tile]
            open_list = [hill_tile]
            hill_tile.dist = 0
            hill_tile.is_reached = True
            head = 0
            while head < len(open_list):
                tile = open_list[head]
                head += 1
                if tile.dist >= 20:
                    break
                for n in tile.neighbors:
                    if n.is_reached:
                        continue
                    n.is_reached = True
                    if n.type == MY_ANT:
                        if not n.ant.has_moved and tile.type != MY_ANT and self._is_tile_safe(n.ant, tile):
                            self._do_move(n, tile)
                        count -= 1
                    n.dist = tile.dist + 1
                    changed.append(n)
                    open_list.append(n)
                if count <= 0:
                    break
            for t in changed:
                t.is_reached = False

    def _food(self) -> None:
        enemy_near: dict[Tile, bool] = {}
        open_list: list[Tile] = []
        changed: list[Tile] = []
        for food in self.foods:
            open_list.append(food)
            food.dist = 0
            food.is_reached = True
            food.source = food
            enemy_near[food] = False
            changed.append(food)
        head = 0
        while head < len(open_list):
            tile = open_list[head]
            head += 1
            if not tile.is_reached:
                continue        # dropped with its source
            if tile.dist <= 2 and tile.is_enemy():
                enemy_near[tile.source] = True
            if tile.dist > 2 and enemy_near[tile.source]:
                src = tile.source
                changed = [t for t in changed if not (t.source is src and not _unreach(t))]
            elif (tile.type == MY_ANT and not tile.ant.has_moved
                  and tile.prev.type != MY_ANT and not self._is_suicide(tile.ant, tile.prev)):
                if tile.prev is tile.source:
                    tile.ant.has_moved = True
                else:
                    enemy_ant = self._is_tile_safe2(tile.ant, tile.prev)
                    if enemy_ant is not None:
                        close = self._get_close_ant_dists(tile.source)
                        i_have_backup = False
                        for close_ant in close:
                            if close_ant is enemy_ant or close_ant is tile.ant:
                                continue
                            if close_ant.tile.type == MY_ANT:
                                i_have_backup = True
                            break
                        if i_have_backup:
                            self._do_move(tile, tile.prev)
                    else:
                        self._do_move(tile, tile.prev)
                src = tile.source
                changed = [t for t in changed if not (t.source is src and not _unreach(t))]
            elif tile.dist < 13:
                for n in tile.neighbors:
                    if n.is_reached:
                        continue
                    n.is_reached = True
                    n.prev = tile
                    n.dist = tile.dist + 1
                    n.source = tile.source
                    changed.append(n)
                    open_list.append(n)
        for t in changed:
            t.is_reached = False

    # ---- attack, escape ----------------------------------------------------------------

    def _attack_detached_enemies(self) -> None:
        if self.is_timeout:
            return
        for enemy in self.enemy_ants_ordered:
            if enemy.is_detached and len(enemy.close_enemy_dists) > 2:
                self._attack_enemy(enemy)

    def _escape_enemies(self) -> None:
        for ant in self.dangered_ants:
            if not ant.has_moved:
                self._escape_ant(ant)

    def _escape_ant(self, my_ant: Ant) -> None:
        check_dist = 8
        values: dict[Tile, int] = {}
        open_list: list[Tile] = []
        changed: list[Tile] = []
        ant_tile = my_ant.tile
        ant_tile.is_reached = True
        ant_tile.dist = 0
        changed.append(ant_tile)
        for n in ant_tile.neighbors:
            values[n] = 0
            open_list.append(n)
            n.dist = 1
            n.is_reached = True
            n.prev_firsts.add(n)
            changed.append(n)
        head = 0
        while head < len(open_list):
            tile = open_list[head]
            head += 1
            if tile.dist >= check_dist:
                break
            for n in tile.neighbors:
                if n.is_reached:
                    if n.dist == tile.dist + 1:
                        n.prev_firsts |= tile.prev_firsts
                    continue
                n.is_reached = True
                n.prev = tile
                n.dist = tile.dist + 1
                n.prev_firsts |= tile.prev_firsts
                changed.append(n)
                open_list.append(n)
        for tile in changed:
            add = check_dist + 1 - tile.dist
            if tile.type == MY_ANT:
                add *= 3
            elif tile.is_enemy():
                add *= -3
            for pf in tile.prev_firsts:
                values[pf] = values[pf] + add
            tile.is_reached = False
            tile.prev_firsts = set()
        best_value = -(1 << 30)
        best_dest = None
        for dest, value in values.items():
            if dest.is_free() and self._is_tile_safe(my_ant, dest) and value > best_value:
                best_value = value
                best_dest = dest
        if best_value != 0 and best_dest is not None:
            self._do_move(ant_tile, best_dest)
        else:
            if not self._is_suicide(my_ant, my_ant.tile):
                my_ant.has_moved = True
            else:
                for dest in my_ant.tile.neighbors:
                    if dest.is_free() and not self._is_suicide(my_ant, dest):
                        self._do_move(my_ant.tile, dest)
                        break

    # ---- areas -------------------------------------------------------------------------

    def _create_areas(self) -> None:
        area_map: dict[Tile, Area] = {}
        open_list: list[Tile] = [a.tile for a in self.enemy_ants]
        for a in self.my_ants:
            open_list.append(a.tile)
            a.tile.is_in_my_area = True
        changed: list[Tile] = []
        for tile in open_list:
            tile.dist = 0
            tile.is_reached = True
            changed.append(tile)
            tile.start_tile = tile
            area = Area()
            if tile.is_hill and tile.hill_player == MY_ANT:
                area.contains_hill = True
                area.hill = tile
            area.player = tile.type
            area.ants.append(tile.ant)
            area.tiles.append(tile)
            area_map[tile] = area
        head = 0
        while head < len(open_list):
            tile = open_list[head]
            head += 1
            if tile.dist >= AREA_DIST:
                break
            for n in tile.neighbors:
                if n.is_reached:
                    if (n.start_tile is not tile.start_tile and n.start_tile.type == tile.start_tile.type
                            and area_map[n.start_tile] is not area_map[tile.start_tile]):
                        n_area = area_map[n.start_tile]
                        t_area = area_map[tile.start_tile]
                        t_area.ants.extend(n_area.ants)
                        t_area.tiles.extend(n_area.tiles)
                        for ant in n_area.ants:
                            area_map[ant.tile] = t_area
                        if n_area.contains_hill:
                            t_area.contains_hill = True
                            t_area.hill = n_area.hill
                else:
                    n.is_reached = True
                    n.dist = tile.dist + 1
                    n.start_tile = tile.start_tile
                    area_map[n.start_tile].tiles.append(n)
                    if n.start_tile.type == MY_ANT:
                        n.is_in_my_area = True
                    if n.is_hill and n.hill_player == MY_ANT:
                        area_map[n.start_tile].contains_hill = True
                        area_map[n.start_tile].hill = n
                    changed.append(n)
                    open_list.append(n)
        for t in changed:
            t.is_reached = False

        self.areas = []
        self.fight_areas = []
        for area in area_map.values():
            if area.is_checked:
                continue
            area.is_checked = True
            self.areas.append(area)
            if area.player == MY_ANT and (area.contains_hill or len(area.ants) >= 5):
                self.fight_areas.append(area)

        for area in self.fight_areas:
            for tile in area.tiles:
                for n in tile.neighbors:
                    if n.type != WATER and not n.is_in_my_area:
                        tile.dist = 0
                        area.border.append(tile)
                        tile.is_border = True
                        break

    def _clean_areas(self) -> None:
        for area in self.areas:
            if area.player == MY_ANT:
                for tile in area.tiles:
                    tile.is_in_my_area = False

    # ---- the fight ---------------------------------------------------------------------

    def _fight(self) -> None:
        for area in self.fight_areas:
            for start_ant in area.ants:
                if (start_ant.has_moved or not start_ant.gamma_dist_enemies
                        or start_ant.num_close_enemies == 0 or start_ant.is_gamma_grouped):
                    continue
                group = self._find_gamma_group(start_ant)
                self._create_dep_tables(group)
                for ant in group.my_ants:
                    if not ant.check_neighbors:
                        ant.is_grouped = True
                for ant in group.my_ants:
                    if ant.is_grouped:
                        continue
                    if self.is_timeout:
                        break
                    self._find_prec_group(ant)
                    if len(self.my_ants_prec) <= 1 or not self.enemy_ants_prec:
                        continue
                    for a in self.my_ants_prec:
                        a.dist_map = {}
                        enemy_tile = a.closest_enemy_tile
                        dy = self.dist_row(a.tile, enemy_tile)
                        dx = self.dist_col(a.tile, enemy_tile)
                        a.dist_map[a.tile] = dx * dx + dy * dy
                        for n in a.tile.neighbors:
                            dy = self.dist_row(n, enemy_tile)
                            dx = self.dist_col(n, enemy_tile)
                            a.dist_map[n] = dx * dx + dy * dy
                        if a.check_all:
                            continue
                        for b in self.my_ants_prec:
                            if a is b:
                                continue
                            if any(n is b.tile for n in a.tile.neighbors):
                                a.check_all = True
                                break
                        if a.check_all:
                            a.check_neighbors = list(a.tile.neighbors)
                    for my_ant in self.my_ants_prec:
                        my_ant.tile.ant = None
                        my_ant.tile.type = LAND
                    self.best_prec_value = -(1 << 30)
                    self.be_aggressive = group.max_num_close_own_ants >= 14
                    self._max_multi(0, 0)
                    for my_ant in self.my_ants_prec:
                        if my_ant.best_to is not None:
                            self._do_move2(my_ant.tile, my_ant.best_to)
                        else:
                            my_ant.has_moved = True
                            my_ant.tile.ant = my_ant
                            my_ant.tile.type = MY_ANT

    def _approach_enemies(self) -> None:
        for area in self.fight_areas:
            for ant in area.ants:
                if ant.has_moved or ant.is_indirectly_dangered or ant.num_close_enemies == 0:
                    continue
                enemy_tile = ant.closest_enemy.tile
                dest = self._a_star(ant.tile, enemy_tile, True)
                if dest is not None:
                    self._do_move(ant.tile, dest)
                elif not self._explore_ant(ant):
                    dest = self._a_star2(ant.tile, enemy_tile, True)
                    if dest is not None:
                        self._do_move(ant.tile, dest)

    def _find_gamma_group(self, start_ant: Ant) -> GammaGroup:
        group = GammaGroup()
        my_open = [start_ant]
        enemy_open: list[Ant] = []
        group.my_ants.append(start_ant)
        start_ant.is_gamma_grouped = True
        while my_open or enemy_open:
            if my_open:
                my_ant = my_open.pop(0)
                group.max_num_close_own_ants = max(group.max_num_close_own_ants, my_ant.num_close_own_ants)
                for enemy in my_ant.gamma_dist_enemies:
                    if enemy.is_gamma_grouped:
                        continue
                    enemy.is_gamma_grouped = True
                    enemy_open.append(enemy)
                    group.enemy_ants.append(enemy)
            if enemy_open:
                enemy = enemy_open.pop(0)
                for my_ant in enemy.gamma_dist_enemies:
                    if my_ant.is_gamma_grouped or my_ant.has_moved:
                        continue
                    my_ant.is_gamma_grouped = True
                    my_open.append(my_ant)
                    group.my_ants.append(my_ant)
        return group

    def _find_prec_group(self, start_ant: Ant) -> None:
        my_open = [start_ant]
        my_changed: list[Ant] = []
        enemy_changed: list[Ant] = []
        start_ant.is_reached = True
        self.my_ants_prec = []
        self.enemy_ants_prec = []
        self.my_move_safe_count = 0
        self.enemy_safe_count = 0
        my_move_max = 20
        while my_open:
            my_ant = my_open.pop(0)
            my_move_test_count = len(my_ant.check_neighbors) if my_ant.check_neighbors is not None else 0
            does_fit = True
            new_enemies: list[Ant] = []
            enemy_test_count = 0
            if self.my_move_safe_count + my_move_test_count > my_move_max:
                does_fit = False
            else:
                for e in my_ant.gamma_dist_enemies:
                    if e.is_reached:
                        continue
                    enemy_changed.append(e)
                    e.is_reached = True
                    new_enemies.append(e)
                    if not e.will_stay:
                        enemy_test_count += 1
                if self.my_move_safe_count + my_move_test_count > 17 - 3 and self.enemy_safe_count + enemy_test_count > 5 - 1:
                    does_fit = False
                elif self.my_move_safe_count + my_move_test_count > 13 - 3 and self.enemy_safe_count + enemy_test_count > 8 - 1:
                    does_fit = False
            if does_fit:
                self.my_move_safe_count += my_move_test_count
                self.enemy_safe_count += enemy_test_count
                self.my_ants_prec.append(my_ant)
                self.enemy_ants_prec.extend(new_enemies)
                my_ant.is_grouped = True
                test: dict[int, Ant] = {}
                for e in new_enemies:
                    for m in e.gamma_dist_enemies:
                        m.comp_value = self.dist(m.tile, start_ant.tile)
                        test[id(m)] = m
                # comp: by distance to the start ant, a random draw breaking ties
                for m in sorted(test.values(), key=lambda a: (a.comp_value, self.turn_random.next_int(100))):
                    if m.has_moved or m.is_grouped or m.is_reached:
                        continue
                    m.is_reached = True
                    my_open.append(m)
                    my_changed.append(m)
            else:
                for e in new_enemies:
                    e.is_reached = False
        for ant in enemy_changed:
            ant.is_reached = False
        for ant in my_changed:
            if not ant.is_grouped:
                ant.is_reached = False

    def _attack_enemy(self, enemy: Ant) -> None:
        self.my_ants_prec = []
        open_list = [enemy.tile]
        changed = [enemy.tile]
        enemy.tile.dist = 0
        enemy.tile.is_reached = True
        count = 0
        head = 0
        while head < len(open_list):
            tile = open_list[head]
            head += 1
            if tile.type == MY_ANT and not tile.ant.has_moved:
                if tile.prev.is_free() and self._is_tile_safe(tile.ant, tile.prev):
                    self._do_move(tile, tile.prev)
                elif self.is_gamma_dist(tile, enemy.tile) and len(tile.ant.gamma_dist_enemies) <= 1:
                    self.my_ants_prec.append(tile.ant)
                count += 1
                if count > 5:
                    break
            if tile.dist >= 10:
                break
            for n in tile.neighbors:
                if n.is_reached:
                    continue
                n.is_reached = True
                n.prev = tile
                n.dist = tile.dist + 1
                changed.append(n)
                open_list.append(n)
        for t in changed:
            t.is_reached = False

        if self.my_ants_prec:
            self.best_prec_value = -(1 << 30)
            self.enemy_ants_prec = [enemy]
            mx = max(a.num_close_own_ants for a in self.my_ants_prec)
            self.be_aggressive = mx >= 6
            self._max_single(0)
            for my_ant in self.my_ants_prec:
                if my_ant.best_to is not None:
                    self._do_move(my_ant.tile, my_ant.best_to)
                else:
                    my_ant.has_moved = True

    def _spend(self) -> None:
        """One leaf evaluated. Past the budget the search is cut the way the Java's clock cuts
        it: each level returns at once and undoes its own moves on the way out."""
        self.evaluations += 1
        if self.evaluations > SEARCH_BUDGET:
            self.is_timeout = True

    def _max_single(self, i: int) -> None:
        if i < len(self.my_ants_prec):
            if self.is_timeout:
                return
            my_ant = self.my_ants_prec[i]
            frm = my_ant.tile
            curr = frm
            ln = len(frm.neighbors)
            if ln > 0:
                index = self.turn_random.next_int(ln)
                for _ in range(ln):
                    index -= 1
                    if index == -1:
                        index = ln - 1
                    n = frm.neighbors[index]
                    if not n.is_free() or n.has_virt_ant:
                        continue
                    self._simple_move(curr, n, my_ant)
                    curr = n
                    n.has_virt_ant = True
                    my_ant.curr_to = n
                    self._max_single(i + 1)
                    n.has_virt_ant = False
            if curr is not frm:
                self._simple_move(curr, frm, my_ant)
            if not frm.has_virt_ant:
                frm.has_virt_ant = True
                my_ant.curr_to = None
                self._max_single(i + 1)
                frm.has_virt_ant = False
        else:
            value = self._min_single()
            if value > self.best_prec_value:
                self.best_prec_value = value
                for ant in self.my_ants_prec:
                    ant.best_to = ant.curr_to

    def _min_single(self) -> int:
        enemy = self.enemy_ants_prec[0]
        frm = enemy.tile
        curr = frm
        best = self._evaluate_single()
        if not enemy.will_stay:
            for n in list(enemy.tile.neighbors):
                if not n.is_free():
                    continue
                self._simple_move(curr, n, enemy)
                curr = n
                value = self._evaluate_single()
                if value < best:
                    best = value
            self._simple_move(curr, frm, enemy)
        return best

    def _evaluate_single(self) -> int:
        self._spend()
        enemy = self.enemy_ants_prec[0]
        can_attack = False
        value = 0
        for my_ant in self.my_ants_prec:
            if not can_attack:
                value -= self.dist(my_ant.tile, enemy.tile)
            if self.is_alpha_dist(my_ant.tile, enemy.tile):
                if can_attack:
                    return 10000
                can_attack = True
        if can_attack:
            return 5000 if self.be_aggressive else -5000
        return value

    def _create_dep_tables(self, group: GammaGroup) -> None:
        for my_ant in group.my_ants:
            my_ant.check_neighbors = []
            my_ant.check_all = False
            for enemy in group.enemy_ants:
                if enemy.will_stay:
                    for my_n in my_ant.tile.neighbors:
                        if self.is_beta_dist(my_n, enemy.tile) and my_n not in my_ant.check_neighbors:
                            my_ant.check_neighbors.append(my_n)
                else:
                    for enemy_n in enemy.tile.neighbors:
                        if self.is_alpha_dist(enemy_n, my_ant.tile):
                            my_ant.check_all = True
                            for n in my_ant.tile.neighbors:
                                my_ant.check_neighbors.append(n)
                            break
                if len(my_ant.check_neighbors) == len(my_ant.tile.neighbors):
                    break
        for enemy in group.enemy_ants:
            if enemy.will_stay:
                for my_ant in group.my_ants:
                    if not my_ant.check_all:
                        for my_n in my_ant.tile.neighbors:
                            if self.is_alpha_dist(my_n, enemy.tile) and my_n not in my_ant.check_neighbors:
                                my_ant.check_neighbors.append(my_n)
            else:
                enemy.dep_table = {}
                for my_ant in group.my_ants:
                    for my_n in my_ant.tile.neighbors:
                        if self.is_alpha_dist(my_n, enemy.tile):
                            lst = list(enemy.tile.neighbors)
                            enemy.dep_table[my_n] = lst
                            for enemy_n in enemy.tile.neighbors:
                                enemy_n.is_checked = False
                        else:
                            for enemy_n in enemy.tile.neighbors:
                                if not self.is_alpha_dist(my_n, enemy_n):
                                    continue
                                lst = enemy.dep_table.setdefault(my_n, [])
                                if enemy_n not in lst:
                                    lst.append(enemy_n)
                                if not my_ant.check_all and my_n not in my_ant.check_neighbors:
                                    my_ant.check_neighbors.append(my_n)
                    for enemy_n in enemy.tile.neighbors:
                        enemy_n.is_checked = False
                        if not self.is_alpha_dist(my_ant.tile, enemy_n):
                            continue
                        lst = enemy.dep_table.setdefault(my_ant.tile, [])
                        if enemy_n not in lst:
                            lst.append(enemy_n)

    def _max_multi(self, i: int, dist_value: int) -> None:
        if self.is_timeout:
            return
        if i < len(self.my_ants_prec):
            my_ant = self.my_ants_prec[i]
            frm = my_ant.tile
            curr = frm
            for n in my_ant.check_neighbors:
                if n.has_virt_ant or not n.is_free():
                    continue
                if n.old_ant is not None and n.old_ant.curr_to is frm:
                    continue
                self._simple_move(curr, n, my_ant)
                curr = n
                n.has_virt_ant = True
                my_ant.curr_to = n
                self._max_multi(i + 1, dist_value + my_ant.dist_map[n])
                n.has_virt_ant = False
            if curr is not frm:
                self._simple_move(curr, frm, my_ant)
            if not frm.has_virt_ant:
                frm.has_virt_ant = True
                my_ant.curr_to = None
                self._max_multi(i + 1, dist_value + my_ant.dist_map[frm])
                frm.has_virt_ant = False
        else:
            self.do_cut = False
            value = self._min_multi(0, dist_value)
            if value > self.best_prec_value:
                self.best_prec_value = value
                for ant in self.my_ants_prec:
                    ant.best_to = ant.curr_to

    def _min_multi(self, i: int, dist_value: int) -> int:
        if self.is_timeout:
            return self.best_prec_value
        if i < len(self.enemy_ants_prec):
            enemy = self.enemy_ants_prec[i]
            frm = enemy.tile
            curr = frm
            best = 1 << 30
            if not enemy.will_stay:
                for key, lst in enemy.dep_table.items():
                    if key.has_virt_ant:
                        for dest in lst:
                            if dest.is_checked or dest.has_virt_ant:
                                continue
                            self._simple_move(curr, dest, enemy)
                            curr = dest
                            dest.has_virt_ant = True
                            value = self._min_multi(i + 1, dist_value)
                            dest.has_virt_ant = False
                            if self.do_cut:
                                if curr is not frm:
                                    self._simple_move(curr, frm, enemy)
                                return self.best_prec_value
                            if value < best:
                                best = value
                if curr is not frm:
                    self._simple_move(curr, frm, enemy)
            if not frm.has_virt_ant:
                frm.has_virt_ant = True
                value = self._min_multi(i + 1, dist_value)
                frm.has_virt_ant = False
                if self.do_cut:
                    return self.best_prec_value
                if value < best:
                    best = value
            return best
        result = self._evaluate_multi(dist_value)
        if result < self.best_prec_value:
            self.do_cut = True
        return result

    def _evaluate_multi(self, dist_value: int) -> int:
        self._spend()
        my_dead = 0
        enemy_dead = 0
        for enemy in self.enemy_ants_prec:
            enemy.is_dead = False
            enemy.weakness = 0
            for my_ant in enemy.gamma_dist_enemies:
                if self.is_alpha_dist(enemy.tile, my_ant.tile):
                    enemy.weakness += 1
                    my_ant.weakness += 1
        for my_ant in self.my_ants_prec:
            if my_ant.weakness != 0:
                for enemy in my_ant.gamma_dist_enemies:
                    if enemy.weakness == 0 or not self.is_alpha_dist(my_ant.tile, enemy.tile):
                        continue
                    if not enemy.is_dead and enemy.weakness >= my_ant.weakness:
                        enemy.is_dead = True
                        enemy_dead += 1
                    if not my_ant.is_dead and my_ant.weakness >= enemy.weakness:
                        my_ant.is_dead = True
                        my_dead += 1
                my_ant.is_dead = False
                my_ant.weakness = 0
        if self.be_aggressive:
            return enemy_dead * 300 - my_dead * 180 - dist_value
        return enemy_dead * 512 - my_dead * (512 + 256) - dist_value

    @staticmethod
    def _simple_move(frm: Tile, to: Tile, ant: Ant) -> None:
        to.type = frm.type
        frm.type = LAND
        ant.tile = to

    # ---- explore and distribute --------------------------------------------------------

    def _init_explore(self) -> None:
        open_list = [a.tile for a in self.my_ants]
        changed: list[Tile] = []
        for tile in open_list:
            tile.dist = 0
            tile.is_reached = True
            tile.start_tile = tile
            changed.append(tile)
        head = 0
        while head < len(open_list):
            tile = open_list[head]
            head += 1
            if tile.dist > 10:
                break
            tile.explore_value = 0
            for n in tile.neighbors:
                if n.is_reached:
                    continue
                n.is_reached = True
                n.prev = tile
                n.dist = tile.dist + 1
                n.start_tile = tile.start_tile
                changed.append(n)
                open_list.append(n)
        for t in changed:
            t.is_reached = False

    def _explore(self) -> None:
        for ant in self.my_ants:
            if ant.has_moved or ant.is_indirectly_dangered:
                continue
            self._explore_ant(ant)

    def _explore_ant(self, ant: Ant) -> bool:
        values: dict[Tile, int] = {}
        open_list: list[Tile] = []
        changed: list[Tile] = []
        ant_tile = ant.tile
        ant_tile.is_reached = True
        ant_tile.dist = 0
        changed.append(ant_tile)
        for n in ant_tile.neighbors:
            values[n] = 0
            open_list.append(n)
            n.dist = 1
            n.is_reached = True
            n.prev_firsts.add(n)
            changed.append(n)
        head = 0
        while head < len(open_list):
            tile = open_list[head]
            head += 1
            if tile.dist > 10:
                for pf in tile.prev_firsts:
                    values[pf] = values[pf] + tile.explore_value
                continue
            for n in tile.neighbors:
                if n.is_reached:
                    if n.dist == tile.dist + 1:
                        n.prev_firsts |= tile.prev_firsts
                    continue
                n.is_reached = True
                n.prev = tile
                n.dist = tile.dist + 1
                n.prev_firsts |= tile.prev_firsts
                changed.append(n)
                open_list.append(n)
        best_value = 0
        best_dest = None
        for dest, value in values.items():
            if dest.is_free() and not dest.is_hill and value > best_value:
                best_value = value
                best_dest = dest
        if best_value == 0 or best_dest is None:
            for t in changed:
                t.is_reached = False
                t.prev_firsts = set()
            return False
        for t in changed:
            if t.dist > 10 and best_dest in t.prev_firsts:
                t.explore_value = 0
            t.is_reached = False
            t.prev_firsts = set()
        self._do_move(ant_tile, best_dest)
        return True

    def _distribute(self, only_near_enemy: bool) -> None:
        for ant in self.my_ants:
            if not ant.has_moved and (ant.num_close_enemies > 0 or not only_near_enemy):
                self._distribute_ant(ant)

    def _distribute_ant(self, ant: Ant) -> None:
        if ant.has_moved:
            return
        close_dist = 2 * self.distribute_check_dist + 2
        close = self._get_close_ant_tiles(ant.tile, close_dist)
        best_n = None
        best_value = -(1 << 30)
        for dest in ant.tile.neighbors:
            if not dest.is_free() or dest.is_hill or not self._is_tile_safe(ant, dest):
                continue
            value = self._calc_space_value_e(dest, close)
            if value > best_value:
                best_value = value
                best_n = dest
        if best_n is not None:
            self._do_move(ant.tile, best_n)

    def _calc_space_value_e(self, dest: Tile, close: list[Tile]) -> int:
        value = 0
        open_list: list[Tile] = []
        changed: list[Tile] = []
        close.append(dest)
        for ant_tile in close:
            open_list.append(ant_tile)
            ant_tile.dist = 0
            ant_tile.is_reached = True
            ant_tile.is_reached_by_me = ant_tile is dest or ant_tile.type == MY_ANT
            changed.append(ant_tile)
        close.pop()
        head = 0
        while head < len(open_list):
            tile = open_list[head]
            head += 1
            if tile.dist >= self.distribute_check_dist:
                break
            for n in tile.neighbors:
                if n.is_reached:
                    continue
                n.is_reached = True
                n.dist = tile.dist + 1
                n.is_reached_by_me = tile.is_reached_by_me
                if n.is_reached_by_me:
                    value += 10 + self.distribute_check_dist - n.dist
                changed.append(n)
                open_list.append(n)
        for t in changed:
            t.is_reached = False
        return value

    # ---- paths -------------------------------------------------------------------------

    def _a_star(self, frm: Tile, to: Tile, start_pyt: bool) -> Optional[Tile]:
        result = None
        max_dist = 3 * self.dist(frm, to)
        open_list: list[Tile] = [to]
        changed: list[Tile] = [to]
        to.f = 0
        to.dist = 0
        to.is_reached = True
        while open_list:
            tile = open_list.pop(0)
            if tile.dist >= max_dist:
                continue
            for n in tile.neighbors:
                if n is frm:
                    if tile.is_free():
                        result = tile
                    break
                if n.is_reached or not n.is_free() or (n.is_hill and n.hill_player == MY_ANT):
                    continue
                n.dist = tile.dist + 1
                n.f = n.dist + self.dist(n, frm)
                if start_pyt and tile is frm and frm.ant is not None and frm.ant.closest_enemy_tile is not None:
                    dx = self.dist_col(n, to)
                    dy = self.dist_row(n, to)
                    n.f = int(np.floor(np.sqrt(dx * dx + dy * dy)))
                if n.f > max_dist:
                    continue
                n.is_reached = True
                changed.append(n)
                _insert_by_f(open_list, n)
            if result is not None:
                break
        for t in changed:
            t.is_reached = False
        return result

    def _a_star2(self, frm: Tile, to: Tile, first_free: bool) -> Optional[Tile]:
        found = False
        max_dist = 400
        open_list: list[Tile] = [frm]
        changed: list[Tile] = [frm]
        frm.f = 0
        frm.dist = 0
        frm.is_reached = True
        frm.prev = None
        while open_list:
            tile = open_list.pop(0)
            if tile.dist >= max_dist:
                continue
            for n in tile.neighbors:
                if (n.is_reached
                        or (tile is frm and ((first_free and not n.is_free()) or not self._is_tile_safe(frm.ant, n)))
                        or (n.is_hill and n.hill_player == MY_ANT)):
                    continue
                n.dist = tile.dist + 1
                n.f = n.dist + self.dist(n, to)
                n.prev = tile
                if n is to:
                    found = True
                    break
                if n.f > max_dist:
                    continue
                n.is_reached = True
                changed.append(n)
                _insert_by_f(open_list, n)
            if found:
                break
        for t in changed:
            t.is_reached = False
        if not found:
            return None
        tile = to
        while tile.prev is not frm:
            tile = tile.prev
        return tile

    def _get_close_ant_tiles(self, t: Tile, d: int) -> list[Tile]:
        return [a.tile for a in self.ants if a.tile is not t and self.dist(t, a.tile) <= d]

    def _get_close_ant_dists(self, t: Tile) -> list[Ant]:
        """`getCloseAntDists(t).values()`: a TreeMap by distance, at most three, so the ants
        found first at each distance, in distance order."""
        close: dict[int, Ant] = {}
        for ant in self.ants:
            d = self.dist(t, ant.tile)
            if d < 8:
                close[d] = ant
                if len(close) >= 3:
                    break
        return [close[k] for k in sorted(close)]

    # ---- moves and safety --------------------------------------------------------------

    def _do_move2(self, frm: Tile, to: Tile) -> None:
        ant = frm.old_ant
        if ant is None:
            return
        if not self.is_mission_phase and ant.has_mission:
            ant.mission.is_removed = True
            ant.has_mission = False
        to.type = MY_ANT
        to.ant = ant
        ant.has_moved = True
        ant.tile = to
        self.issue_order(frm, frm.dir_to(to, self.rows, self.cols))

    def _do_move(self, frm: Tile, to: Tile) -> None:
        ant = frm.old_ant
        if ant is None:
            return
        if not self.is_mission_phase and ant.has_mission:
            ant.mission.is_removed = True
            ant.has_mission = False
        to.type = frm.type
        to.ant = ant
        frm.type = LAND
        frm.ant = None
        ant.has_moved = True
        ant.tile = to
        self.issue_order(frm, frm.dir_to(to, self.rows, self.cols))

    def _is_tile_safe(self, ant: Ant, dest: Tile) -> bool:
        for enemy in ant.gamma_dist_enemies:
            if enemy.will_stay:
                if self.is_alpha_dist(enemy.tile, dest):
                    return False
            elif self.is_beta_dist(enemy.tile, dest):
                if len(enemy.tile.neighbors) == 4:
                    return False
                for n in enemy.tile.neighbors:
                    if self.is_alpha_dist(n, dest):
                        return False
        return True

    def _is_tile_safe2(self, ant: Ant, dest: Tile) -> Optional[Ant]:
        for enemy in ant.gamma_dist_enemies:
            if enemy.will_stay:
                if self.is_alpha_dist(enemy.tile, dest):
                    return enemy
            elif self.is_beta_dist(enemy.tile, dest):
                if len(enemy.tile.neighbors) == 4:
                    return enemy
                for n in enemy.tile.neighbors:
                    if self.is_alpha_dist(n, dest):
                        return enemy
        return None

    def _is_suicide(self, ant: Ant, dest: Tile) -> bool:
        dangered = False
        for enemy in ant.gamma_dist_enemies:
            if enemy.will_stay:
                if self.is_alpha_dist(enemy.tile, dest):
                    if dangered:
                        return True
                    dangered = True
            elif self.is_beta_dist(enemy.tile, dest):
                if len(enemy.tile.neighbors) == 4:
                    if dangered:
                        return True
                    dangered = True
                else:
                    for n in enemy.tile.neighbors:
                        if self.is_alpha_dist(n, dest):
                            if dangered:
                                return True
                            dangered = True
        return False

    # ---- what the bot remembers, for a model -------------------------------------------

    def memory_state(self) -> dict:
        """The three things kept between turns, as `collect.py` writes them under `m` for the
        xathis column: `explore` (each tile's exploreValue, capped at 255, row-major), `stay`
        (each tile's stay detector packed as mask + 16 x min(count, 15), 0 where no ant stands)
        and `missions` (one `[row, col, has, target_row, target_col, age]` an ant with one)."""
        rows, cols = self.rows, self.cols
        explore = np.zeros((rows, cols), dtype=np.uint8)
        stay = np.zeros((rows, cols), dtype=np.uint8)
        for r in range(rows):
            row = self.map[r]
            for c in range(cols):
                t = row[c]
                explore[r, c] = min(t.explore_value, 255)
                if t.ant is not None and t.stay_value >= 0:
                    stay[r, c] = (t.stay_value & 15) + 16 * min(t.stay_turn_count, 15)
        missions = []
        for m in self.missions:
            if m.is_removed or m.curr_tile is None or m.target is None:
                continue
            missions.append([m.curr_tile.row, m.curr_tile.col, m.target.row, m.target.col,
                             min(self.turn - m.last_updated, 255)])
        return {"explore": explore, "stay": stay, "missions": missions}

    def missions_by_ant(self) -> list[list[int]]:
        """After a turn, one `[has, target_row, target_col]` an ant in `mine`'s order (the order
        `self.my_ants` keeps): the mission the ant leaves the turn with, or zeros. A model with a
        memory per ant is taught to write this and read it back."""
        out = []
        for ant in self.my_ants:
            m = ant.mission
            if ant.has_mission and m is not None and not m.is_removed and m.target is not None:
                out.append([1, m.target.row, m.target.col])
            else:
                out.append([0, 0, 0])
        return out


def _unreach(t: Tile) -> bool:
    t.is_reached = False
    return False


def _insert_by_f(open_list: list[Tile], n: Tile) -> None:
    """The Java's ordered insert: before the first entry whose `f` is larger, else at the end."""
    for i, t in enumerate(open_list):
        if t.f > n.f:
            open_list.insert(i, n)
            return
    open_list.append(n)

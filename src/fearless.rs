//! Fearless draft: champions picked in earlier games of a series become unavailable.
//!
//! The state is kept with the tree (and saved with it), not as tree nodes: one row of ten
//! champions per earlier game. In each row the first five slots are the picks of the team
//! that is on **blue side in the current game**, the last five those of the team on red
//! side, so soft fearless never needs to track side swaps between games.
//!
//! - **Hard fearless** locks every champion of every earlier game for both teams, for picks
//!   and bans alike.
//! - **Soft fearless** only stops a team from picking its own earlier champions again;
//!   bans are not affected.
//!
//! Rows for games after the current one are kept (and saved), so going back up a game
//! number restores them without retyping.

use crate::champions;
use crate::draft::{Action, DraftPos, Team};

pub const MAX_GAME: u8 = 7;
/// Rows for earlier games: game 7 has six.
pub const ROWS: usize = MAX_GAME as usize - 1;
pub const SLOTS: usize = 10;
pub const HALF: usize = SLOTS / 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Hard,
    Soft,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Hard => "Hard",
            Mode::Soft => "Soft",
        }
    }
}

/// Why a champion (or a branch) is unavailable: hard fearless (both teams) or soft fearless
/// for one team. Also picks the color it is shown in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tone {
    Both,
    Blue,
    Red,
}

impl Tone {
    pub fn class(self) -> &'static str {
        match self {
            Tone::Both => "fl-both",
            Tone::Blue => "fl-blue",
            Tone::Red => "fl-red",
        }
    }
}

/// A set of champion values as a 256-bit mask.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LockSet([u64; 4]);

impl LockSet {
    pub fn insert(&mut self, value: u8) {
        self.0[usize::from(value / 64)] |= 1 << (value % 64);
    }

    pub fn contains(&self, value: u8) -> bool {
        self.0[usize::from(value / 64)] & (1 << (value % 64)) != 0
    }

    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|&w| w == 0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fearless {
    /// The game being prepared, 1-7. Game 1 has no fearless bans.
    pub game: u8,
    pub mode: Mode,
    /// `rows[g]` holds the picks of game `g + 1`; `0` is an empty slot.
    pub rows: [[u8; SLOTS]; ROWS],
}

impl Default for Fearless {
    fn default() -> Self {
        Fearless {
            game: 1,
            mode: Mode::Hard,
            rows: [[0; SLOTS]; ROWS],
        }
    }
}

impl Fearless {
    /// Rows that count for the current game.
    pub fn active_rows(&self) -> &[[u8; SLOTS]] {
        &self.rows[..usize::from(self.game.clamp(1, MAX_GAME) - 1)]
    }

    /// Whether any champion is locked at all.
    pub fn is_active(&self) -> bool {
        self.active_rows().iter().flatten().any(|&v| v != 0)
    }

    /// Champions in the active rows (both halves).
    pub fn count(&self) -> usize {
        self.active_rows()
            .iter()
            .flatten()
            .filter(|&&v| v != 0)
            .count()
    }

    fn half(team: Team) -> std::ops::Range<usize> {
        match team {
            Team::Blue => 0..HALF,
            Team::Red => HALF..SLOTS,
        }
    }

    /// Champions that can't be chosen at `pos`.
    pub fn locked_at(&self, pos: DraftPos) -> LockSet {
        let mut set = LockSet::default();
        if pos == DraftPos::ROOT {
            return set;
        }
        let range = match (self.mode, pos.action()) {
            (Mode::Hard, _) => 0..SLOTS,
            (Mode::Soft, Action::Ban) => return set,
            (Mode::Soft, Action::Pick) => Self::half(pos.team()),
        };
        for row in self.active_rows() {
            for &v in &row[range.clone()] {
                if v != 0 {
                    set.insert(v);
                }
            }
        }
        set
    }

    /// How `champion` is locked, and the first game it was picked in, for the champion grid
    /// (`F1` ... `F6`). In soft mode the tone says which side can't pick it any more.
    pub fn mark(&self, champion: u8) -> Option<(u8, Tone)> {
        let mut first = None;
        let (mut blue, mut red) = (false, false);
        for (g, row) in self.active_rows().iter().enumerate() {
            for (i, &v) in row.iter().enumerate() {
                if v == champion {
                    first.get_or_insert(g as u8 + 1);
                    if i < HALF {
                        blue = true;
                    } else {
                        red = true;
                    }
                }
            }
        }
        let tone = match (self.mode, blue, red) {
            (Mode::Hard, _, _) | (Mode::Soft, true, true) => Tone::Both,
            (Mode::Soft, true, false) => Tone::Blue,
            _ => Tone::Red,
        };
        first.map(|g| (g, tone))
    }

    /// Whether `champion` may go into `slot` of game `game` (1-based) in the editor:
    /// hard fearless allows each champion once across all rows, soft fearless once per
    /// team side. The slot being edited itself doesn't count.
    pub fn can_place(&self, game: usize, slot: usize, champion: u8) -> bool {
        let same_half = |i: usize| (i < HALF) == (slot < HALF);
        !self.rows.iter().enumerate().any(|(g, row)| {
            row.iter().enumerate().any(|(i, &v)| {
                v == champion
                    && !(g + 1 == game && i == slot)
                    && (self.mode == Mode::Hard || same_half(i))
            })
        })
    }

    // Save format: one flags byte, then 10 bytes per stored row.
    //
    //   bits 0-2  game number (1-7)
    //   bit 3     soft fearless
    //   bits 4-6  number of stored rows (0-6): trailing empty rows are dropped

    /// Bytes for the save format.
    pub fn encode(&self, out: &mut Vec<u8>) {
        let stored = self
            .rows
            .iter()
            .rposition(|row| row.iter().any(|&v| v != 0))
            .map_or(0, |i| i + 1);
        let soft = u8::from(self.mode == Mode::Soft);
        out.push(self.game.clamp(1, MAX_GAME) | soft << 3 | (stored as u8) << 4);
        for row in &self.rows[..stored] {
            out.extend_from_slice(row);
        }
    }

    /// Reads [`encode`](Self::encode)'s bytes, returning the rest.
    pub fn decode(bytes: &[u8]) -> Result<(Fearless, &[u8]), &'static str> {
        let (&flags, mut rest) = bytes.split_first().ok_or("missing fearless data")?;
        let game = flags & 0b111;
        let stored = usize::from(flags >> 4 & 0b111);
        if !(1..=MAX_GAME).contains(&game) || stored > ROWS || flags & 0x80 != 0 {
            return Err("invalid fearless data");
        }
        let mut fearless = Fearless {
            game,
            mode: if flags & 0b1000 != 0 {
                Mode::Soft
            } else {
                Mode::Hard
            },
            ..Fearless::default()
        };
        for row in &mut fearless.rows[..stored] {
            let (bytes, tail) = rest
                .split_first_chunk::<SLOTS>()
                .ok_or("unexpected file length")?;
            if bytes
                .iter()
                .any(|&v| v != 0 && champions::by_value(v).is_none())
            {
                return Err("fearless bans must be champions");
            }
            *row = *bytes;
            rest = tail;
        }
        Ok((fearless, rest))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draft::{Action::*, Team::*};

    fn sample(mode: Mode) -> Fearless {
        let mut f = Fearless {
            game: 3,
            mode,
            ..Fearless::default()
        };
        f.rows[0][0] = 16; // game 1, blue-side team
        f.rows[0][5] = 17; // game 1, red-side team
        f.rows[1][6] = 16; // game 2, red-side team picks it too (soft only)
        f.rows[3][0] = 40; // game 4: kept but not active
        f
    }

    #[test]
    fn hard_locks_everything_for_both_teams() {
        let f = sample(Mode::Hard);
        for pos in [DraftPos::new(Blue, Ban, 1), DraftPos::new(Red, Pick, 2)] {
            let set = f.locked_at(pos);
            assert!(set.contains(16) && set.contains(17));
            assert!(!set.contains(40), "rows after the current game don't count");
        }
        assert_eq!(f.mark(17), Some((1, Tone::Both)));
        assert_eq!(f.mark(40), None);
        assert_eq!(f.count(), 3);
    }

    #[test]
    fn soft_locks_picks_per_side() {
        let f = sample(Mode::Soft);
        assert!(f.locked_at(DraftPos::new(Blue, Ban, 1)).is_empty());
        let blue = f.locked_at(DraftPos::new(Blue, Pick, 1));
        assert!(blue.contains(16) && !blue.contains(17));
        let red = f.locked_at(DraftPos::new(Red, Pick, 1));
        assert!(red.contains(16) && red.contains(17));
        assert_eq!(f.mark(16), Some((1, Tone::Both)));
        assert_eq!(f.mark(17), Some((1, Tone::Red)));
    }

    #[test]
    fn editor_placement_rules() {
        let hard = sample(Mode::Hard);
        assert!(!hard.can_place(3, 0, 17));
        assert!(hard.can_place(1, 5, 17), "its own slot");
        let soft = sample(Mode::Soft);
        assert!(
            soft.can_place(3, 0, 17),
            "red-side team picked it, blue may too"
        );
        assert!(!soft.can_place(3, 7, 17));
    }

    #[test]
    fn round_trips_and_keeps_later_rows() {
        let f = sample(Mode::Soft);
        let mut bytes = Vec::new();
        f.encode(&mut bytes);
        assert_eq!(bytes.len(), 1 + 4 * SLOTS);
        bytes.push(99);
        let (back, rest) = Fearless::decode(&bytes).unwrap();
        assert_eq!(back, f);
        assert_eq!(rest, [99]);

        let mut none = Vec::new();
        Fearless::default().encode(&mut none);
        assert_eq!(none, [1]);
        assert!(Fearless::decode(&[0]).is_err());
        assert!(Fearless::decode(&[0x11, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0]).is_err());
    }
}

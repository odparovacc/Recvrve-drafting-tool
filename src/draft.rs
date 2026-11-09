//! Standard tournament draft order.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Team {
    Blue,
    Red,
}

impl Team {
    pub fn label(self) -> &'static str {
        match self {
            Team::Blue => "Blue",
            Team::Red => "Red",
        }
    }

    pub fn class(self) -> &'static str {
        match self {
            Team::Blue => "blue",
            Team::Red => "red",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Ban,
    Pick,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Ban => "Ban",
            Action::Pick => "Pick",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Action::Ban => "B",
            Action::Pick => "P",
        }
    }

    pub fn class(self) -> &'static str {
        match self {
            Action::Ban => "ban",
            Action::Pick => "pick",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DraftStep {
    pub team: Team,
    pub action: Action,
    /// 1-based ban or pick number for this team.
    pub number: u8,
}

impl DraftStep {
    pub const fn pos(self) -> DraftPos {
        DraftPos::new(self.team, self.action, self.number)
    }

    /// Short code for headers: team, action and number, e.g. `BB1` (blue ban 1) or
    /// `RP5` (red pick 5).
    pub fn code(self) -> String {
        let team = match self.team {
            Team::Blue => 'B',
            Team::Red => 'R',
        };
        format!("{team}{}{}", self.action.short(), self.number)
    }
}

/// A draft position packed into one byte:
///
/// ```text
///  bit 7   bit 6   bits 5..0
///  team    action  number (1-based)
///  0=blue  0=ban
///  1=red   1=pick
/// ```
///
/// Blue ban 2 is `0b0000_0010`, red pick 5 is `0b1100_0101`. The root node uses
/// [`DraftPos::ROOT`] (`0`), which is not a real position since numbers start at 1.
///
/// `#[repr(transparent)]` guarantees this is laid out exactly like a bare `u8`.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DraftPos(u8);

impl DraftPos {
    const TEAM_BIT: u8 = 0b1000_0000;
    const ACTION_BIT: u8 = 0b0100_0000;
    const NUMBER_MASK: u8 = 0b0011_1111;

    /// Position of the root node (the empty draft).
    pub const ROOT: DraftPos = DraftPos(0);

    pub const fn new(team: Team, action: Action, number: u8) -> DraftPos {
        let team_bit = match team {
            Team::Blue => 0,
            Team::Red => Self::TEAM_BIT,
        };
        let action_bit = match action {
            Action::Ban => 0,
            Action::Pick => Self::ACTION_BIT,
        };
        DraftPos(team_bit | action_bit | (number & Self::NUMBER_MASK))
    }

    /// Raw byte.
    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn team(self) -> Team {
        if self.0 & Self::TEAM_BIT == 0 {
            Team::Blue
        } else {
            Team::Red
        }
    }

    pub const fn action(self) -> Action {
        if self.0 & Self::ACTION_BIT == 0 {
            Action::Ban
        } else {
            Action::Pick
        }
    }

    pub const fn number(self) -> u8 {
        self.0 & Self::NUMBER_MASK
    }

    /// Index of this position in [`DRAFT_ORDER`]; `None` for [`DraftPos::ROOT`].
    pub fn step_index(self) -> Option<usize> {
        DRAFT_ORDER.iter().position(|s| s.pos() == self)
    }

    /// Number of draft actions taken once this position is filled: 0 for the root, 1 for
    /// blue ban 1, ..., 20 for red pick 5.
    pub fn depth(self) -> usize {
        self.step_index().map_or(0, |i| i + 1)
    }

    /// The position that follows this one, or `None` after the last pick.
    pub fn next(self) -> Option<DraftPos> {
        DRAFT_ORDER.get(self.depth()).map(|s| s.pos())
    }
}

const fn step(team: Team, action: Action, number: u8) -> DraftStep {
    DraftStep {
        team,
        action,
        number,
    }
}

use Action::{Ban, Pick};
use Team::{Blue, Red};

pub const DRAFT_ORDER: [DraftStep; 20] = [
    // Ban phase 1
    step(Blue, Ban, 1),
    step(Red, Ban, 1),
    step(Blue, Ban, 2),
    step(Red, Ban, 2),
    step(Blue, Ban, 3),
    step(Red, Ban, 3),
    // Pick phase 1
    step(Blue, Pick, 1),
    step(Red, Pick, 1),
    step(Red, Pick, 2),
    step(Blue, Pick, 2),
    step(Blue, Pick, 3),
    step(Red, Pick, 3),
    // Ban phase 2
    step(Red, Ban, 4),
    step(Blue, Ban, 4),
    step(Red, Ban, 5),
    step(Blue, Ban, 5),
    // Pick phase 2
    step(Red, Pick, 4),
    step(Blue, Pick, 4),
    step(Blue, Pick, 5),
    step(Red, Pick, 5),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_codes_follow_the_draft_order() {
        let codes: Vec<String> = DRAFT_ORDER.iter().map(|s| s.code()).collect();
        assert_eq!(
            codes.join(" "),
            "BB1 RB1 BB2 RB2 BB3 RB3 BP1 RP1 RP2 BP2 BP3 RP3 RB4 BB4 RB5 BB5 RP4 BP4 BP5 RP5"
        );
    }

    #[test]
    fn each_team_gets_five_bans_and_five_picks_in_sequence() {
        for team in [Blue, Red] {
            for action in [Ban, Pick] {
                let numbers: Vec<u8> = DRAFT_ORDER
                    .iter()
                    .filter(|s| s.team == team && s.action == action)
                    .map(|s| s.number)
                    .collect();
                assert_eq!(numbers, [1, 2, 3, 4, 5], "{team:?} {action:?}");
            }
        }
    }

    #[test]
    fn position_byte_layout() {
        assert_eq!(DraftPos::new(Blue, Ban, 2).bits(), 0b0000_0010);
        assert_eq!(DraftPos::new(Red, Pick, 5).bits(), 0b1100_0101);
        assert_eq!(DraftPos::new(Blue, Pick, 4).bits(), 0b0100_0100);
        assert_eq!(DraftPos::new(Red, Ban, 1).bits(), 0b1000_0001);

        for s in DRAFT_ORDER {
            let pos = s.pos();
            assert_eq!(
                (pos.team(), pos.action(), pos.number()),
                (s.team, s.action, s.number)
            );
        }
    }

    #[test]
    fn positions_walk_the_draft_order() {
        let mut pos = DraftPos::ROOT;
        assert_eq!(pos.depth(), 0);
        for (i, s) in DRAFT_ORDER.iter().enumerate() {
            pos = pos.next().unwrap();
            assert_eq!(pos, s.pos());
            assert_eq!(pos.depth(), i + 1);
        }
        assert_eq!(pos.next(), None);
        assert_eq!(std::mem::size_of::<DraftPos>(), 1);
    }
}

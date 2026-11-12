// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Action classes (AI control layer addendum, section 5).

use std::fmt;

use serde::{Deserialize, Serialize};

/// The authority class of an action.
///
/// Classes are ordered: a higher class is stricter. Serialised as its number.
///
/// ```
/// use uos_agent::ActionClass;
///
/// assert!(ActionClass::Observe < ActionClass::Prohibited);
/// assert_eq!(ActionClass::from_u8(3), Some(ActionClass::SystemCritical));
/// assert_eq!(ActionClass::from_u8(5), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum ActionClass {
    /// Class 0: reads state and changes nothing.
    Observe,
    /// Class 1: changes session or user state in a way that can be undone.
    Reversible,
    /// Class 2: hard to undo, or affects other people or money.
    Consequential,
    /// Class 3: can destroy data or stop the machine booting.
    SystemCritical,
    /// Class 4: never performed by the assistant.
    Prohibited,
}

impl ActionClass {
    /// Every class, in ascending order.
    pub const ALL: [ActionClass; 5] = [
        ActionClass::Observe,
        ActionClass::Reversible,
        ActionClass::Consequential,
        ActionClass::SystemCritical,
        ActionClass::Prohibited,
    ];

    /// The class number, 0 to 4.
    pub const fn as_u8(self) -> u8 {
        match self {
            ActionClass::Observe => 0,
            ActionClass::Reversible => 1,
            ActionClass::Consequential => 2,
            ActionClass::SystemCritical => 3,
            ActionClass::Prohibited => 4,
        }
    }

    /// The class for a number, or `None` if the number is not 0 to 4.
    pub const fn from_u8(value: u8) -> Option<ActionClass> {
        match value {
            0 => Some(ActionClass::Observe),
            1 => Some(ActionClass::Reversible),
            2 => Some(ActionClass::Consequential),
            3 => Some(ActionClass::SystemCritical),
            4 => Some(ActionClass::Prohibited),
            _ => None,
        }
    }

    /// The class name used in documentation and messages.
    pub const fn name(self) -> &'static str {
        match self {
            ActionClass::Observe => "observe",
            ActionClass::Reversible => "reversible",
            ActionClass::Consequential => "consequential",
            ActionClass::SystemCritical => "system-critical",
            ActionClass::Prohibited => "prohibited",
        }
    }
}

impl fmt::Display for ActionClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_u8())
    }
}

impl From<ActionClass> for u8 {
    fn from(class: ActionClass) -> u8 {
        class.as_u8()
    }
}

/// A number that is not an action class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidClass(pub u8);

impl fmt::Display for InvalidClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} is not an action class (0 to 4)", self.0)
    }
}

impl std::error::Error for InvalidClass {}

impl TryFrom<u8> for ActionClass {
    type Error = InvalidClass;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        ActionClass::from_u8(value).ok_or(InvalidClass(value))
    }
}

#[cfg(any())]
mod tests {
    use super::*;

    #[test]
    fn numbers_round_trip_and_order_matches() {
        for (index, class) in ActionClass::ALL.into_iter().enumerate() {
            assert_eq!(usize::from(class.as_u8()), index);
            assert_eq!(ActionClass::from_u8(class.as_u8()), Some(class));
        }
        for pair in ActionClass::ALL.windows(2) {
            assert!(pair[0] < pair[1]);
        }
        assert_eq!(ActionClass::from_u8(5), None);
        assert_eq!(ActionClass::try_from(9), Err(InvalidClass(9)));
    }

    #[test]
    fn serialises_as_a_number() {
        assert_eq!(
            serde_json::to_string(&ActionClass::Consequential).unwrap(),
            "2"
        );
        assert_eq!(
            serde_json::from_str::<ActionClass>("3").unwrap(),
            ActionClass::SystemCritical
        );
        assert!(serde_json::from_str::<ActionClass>("5").is_err());
        assert!(serde_json::from_str::<ActionClass>("\"3\"").is_err());
    }
}

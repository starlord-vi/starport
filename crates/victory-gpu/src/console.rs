//! Which console the title runs on. Titles pick settings (resolution,
//! memory budgets) by it, so the user chooses; Series X unless told.

use std::sync::atomic::{AtomicBool, Ordering};

const GIB: usize = 1 << 30;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Console {
    #[default]
    SeriesX,
    SeriesS,
}

impl Console {
    /// Title memory in bytes: (GPU-optimal, standard). Series X in the
    /// advanced configuration; Series S has one range, counted as standard
    /// (7,776 MiB, the published console-mode figure).
    pub const fn title_memory(self) -> (usize, usize) {
        match self {
            Console::SeriesX => (10 * GIB, 7 * GIB / 2),
            Console::SeriesS => (0, 7_776 << 20),
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Console::SeriesX => "Xbox Series X",
            Console::SeriesS => "Xbox Series S",
        }
    }

    /// An explicit CLI profile takes precedence over the environment preset.
    pub fn from_environment(self) -> Self {
        match std::env::var("STARPORT_SERIES_S").as_deref() {
            Ok("1") => Self::SeriesS,
            Ok("0") => Self::SeriesX,
            _ => self,
        }
    }
}

static SERIES_S: AtomicBool = AtomicBool::new(false);

/// Set before the title starts.
pub fn set(console: Console) {
    SERIES_S.store(console == Console::SeriesS, Ordering::Relaxed);
}

pub fn current() -> Console {
    if SERIES_S.load(Ordering::Relaxed) {
        Console::SeriesS
    } else {
        Console::SeriesX
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn series_s_uses_one_smaller_title_pool() {
        let (fast, standard) = Console::SeriesS.title_memory();
        assert_eq!(fast, 0);
        assert_eq!(standard, 7_776 << 20);
        let (x_fast, x_standard) = Console::SeriesX.title_memory();
        assert!(standard < x_fast + x_standard);
    }
}

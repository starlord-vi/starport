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
    pub const fn profile_memory(self) -> (usize, usize) {
        match self {
            Console::SeriesX => (10 * GIB, 7 * GIB / 2),
            Console::SeriesS => (0, 7_776 << 20),
        }
    }

    /// Reported pool budgets only: console identity and host DXGI budget
    /// remain distinct. All title-memory queries use this same split.
    pub fn title_memory(self) -> (usize, usize) {
        budgeted(self.profile_memory(), memory_override())
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

fn memory_override() -> Option<usize> {
    static LIMIT: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    *LIMIT.get_or_init(|| {
        let value = std::env::var("STARPORT_MEM_BUDGET_MB").ok()?;
        if value.is_empty() || value == "0" {
            return None;
        }
        let mb = value
            .parse::<usize>()
            .ok()
            .filter(|n| (2048..=65536).contains(n));
        if mb.is_none() {
            tracing::warn!(
                value,
                "invalid memory budget override; using console profile"
            );
        }
        mb.map(|n| (n & !1) << 20)
    })
}
fn budgeted(profile: (usize, usize), limit: Option<usize>) -> (usize, usize) {
    let (fast, standard) = profile;
    let total = fast + standard;
    let Some(limit) = limit.filter(|l| *l < total) else {
        return profile;
    };
    let fast =
        ((fast as u128 * limit as u128 / total.max(1) as u128) as usize) / (2 << 20) * (2 << 20);
    (fast, limit - fast)
}

/// Set before the title starts.
pub fn set(console: Console) {
    SERIES_S.store(console == Console::SeriesS, Ordering::Relaxed);
    let (fast, standard) = console.title_memory();
    tracing::info!(target:"starport::profile",identity=console.name(),override_mb=?memory_override().map(|n|n>>20),
        fast_mb=fast>>20,standard_mb=standard>>20,"title memory budget; host VRAM budget and console identity remain independent");
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
    fn reduced_budget_preserves_pool_proportions_and_console_identity() {
        let x = Console::SeriesX;
        let raw = x.profile_memory();
        let (fast, standard) = budgeted(raw, Some(8192 << 20));
        assert_eq!(fast + standard, 8192 << 20);
        assert!(fast < raw.0 && standard < raw.1);
        assert_eq!(fast % (2 << 20), 0);
        assert_eq!(x.name(), "Xbox Series X");
        assert_eq!(
            budgeted(Console::SeriesS.profile_memory(), Some(6144 << 20)),
            (0, 6144 << 20)
        );
        assert_eq!(budgeted(raw, Some(65536 << 20)), raw);
        assert_eq!(budgeted(raw, None), raw);
    }

    #[test]
    fn series_s_uses_one_smaller_title_pool() {
        let (fast, standard) = Console::SeriesS.title_memory();
        assert_eq!(fast, 0);
        assert_eq!(standard, 7_776 << 20);
        let (x_fast, x_standard) = Console::SeriesX.title_memory();
        assert!(standard < x_fast + x_standard);
    }
}

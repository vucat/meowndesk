//! Suspend-aware monotonic time plus a separate UTC display clock.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

pub trait Clock: Send + Sync {
    /// Boot-relative monotonic milliseconds (Linux CLOCK_BOOTTIME, Windows GetTickCount64).
    fn boot_ms(&self) -> u64;
    /// Wall-clock UTC milliseconds, used for display labels only.
    fn wall_ms(&self) -> i64;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn boot_ms(&self) -> u64 {
        #[cfg(target_os = "linux")]
        {
            // SAFETY: clock_gettime writes a valid timespec for a constant clock id.
            let mut time = libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            let result = unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut time) };
            if result == 0 && time.tv_sec >= 0 && time.tv_nsec >= 0 {
                return (time.tv_sec as u64)
                    .saturating_mul(1000)
                    .saturating_add((time.tv_nsec as u64) / 1_000_000);
            }
            fallback_boot_ms()
        }
        #[cfg(target_os = "windows")]
        {
            // SAFETY: GetTickCount64 takes no pointers and has no preconditions.
            unsafe { windows::Win32::System::SystemInformation::GetTickCount64() }
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            fallback_boot_ms()
        }
    }

    fn wall_ms(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0)
    }
}

#[cfg(not(target_os = "windows"))]
fn fallback_boot_ms() -> u64 {
    use std::sync::OnceLock;
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis() as u64
}

/// Translates a boot-relative timer into elapsed time since the session started.
pub struct SessionClock<C: Clock> {
    source: C,
    origin_boot_ms: u64,
}

impl<C: Clock> SessionClock<C> {
    pub fn new(source: C) -> Self {
        let origin_boot_ms = source.boot_ms();
        Self {
            source,
            origin_boot_ms,
        }
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.source.boot_ms().saturating_sub(self.origin_boot_ms)
    }

    pub fn wall_ms(&self) -> i64 {
        self.source.wall_ms()
    }

    pub fn source(&self) -> &C {
        &self.source
    }
}

/// Injectable clock for deterministic tests; wall time can jump without moving elapsed time.
#[derive(Debug, Default)]
pub struct FakeClock {
    boot_ms: AtomicU64,
    wall_ms: AtomicI64,
}

impl FakeClock {
    pub fn new(boot_ms: u64, wall_ms: i64) -> Self {
        Self {
            boot_ms: AtomicU64::new(boot_ms),
            wall_ms: AtomicI64::new(wall_ms),
        }
    }

    pub fn advance(&self, milliseconds: u64) {
        self.boot_ms.fetch_add(milliseconds, Ordering::SeqCst);
        self.wall_ms
            .fetch_add(milliseconds as i64, Ordering::SeqCst);
    }

    pub fn skip_wall_clock(&self, milliseconds: i64) {
        self.wall_ms.fetch_add(milliseconds, Ordering::SeqCst);
    }
}

impl Clock for FakeClock {
    fn boot_ms(&self) -> u64 {
        self.boot_ms.load(Ordering::SeqCst)
    }

    fn wall_ms(&self) -> i64 {
        self.wall_ms.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_elapsed_ignores_wall_clock_changes() {
        let clock = FakeClock::new(50_000, 1_760_000_000_000);
        let session = SessionClock::new(clock);
        assert_eq!(session.elapsed_ms(), 0);
        session.source().advance(1000);
        session.source().skip_wall_clock(-3_600_000);
        assert_eq!(session.elapsed_ms(), 1000);
        assert_eq!(session.wall_ms(), 1_759_996_401_000);
    }

    #[test]
    fn session_elapsed_cannot_go_negative_after_a_boot_timer_reset() {
        let clock = FakeClock::new(10_000, 0);
        let session = SessionClock::new(clock);
        session.source().advance(500);
        assert_eq!(session.elapsed_ms(), 500);
        session.source().boot_ms.store(10, Ordering::SeqCst);
        assert_eq!(session.elapsed_ms(), 0);
    }
}

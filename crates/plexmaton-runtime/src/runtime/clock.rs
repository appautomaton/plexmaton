use std::time::{Instant, SystemTime, UNIX_EPOCH};

use plexmaton_agent::UnixMillis;

use crate::RuntimeError;

pub(super) trait WallClock: Send + Sync + 'static {
    fn now(&self) -> UnixMillis;
}

pub(super) struct SystemWallClock {
    origin: UnixMillis,
    started: Instant,
}

impl SystemWallClock {
    pub(super) fn new() -> Result<Self, RuntimeError> {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| RuntimeError::WallClockBeforeUnixEpoch)?;
        let millis =
            u64::try_from(elapsed.as_millis()).map_err(|_| RuntimeError::WallClockOutOfRange)?;
        Ok(Self {
            origin: UnixMillis::new(millis),
            started: Instant::now(),
        })
    }
}

impl WallClock for SystemWallClock {
    fn now(&self) -> UnixMillis {
        let elapsed = u64::try_from(self.started.elapsed().as_millis())
            .unwrap_or_else(|_| unreachable!("one process cannot run for u64 milliseconds"));
        let millis = self
            .origin
            .get()
            .checked_add(elapsed)
            .unwrap_or_else(|| unreachable!("validated wall chronology cannot overflow"));
        UnixMillis::new(millis)
    }
}

#[cfg(test)]
pub(super) struct FixedWallClock(pub(super) UnixMillis);

#[cfg(test)]
impl WallClock for FixedWallClock {
    fn now(&self) -> UnixMillis {
        self.0
    }
}

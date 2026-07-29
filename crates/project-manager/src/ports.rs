//! Host port allocation.
//!
//! Two independent guarantees:
//!
//! * The database's `UNIQUE (host_port, protocol, bind_address)` constraint
//!   makes double allocation impossible even under concurrent creation.
//! * A real bind test before use catches ports held by something outside
//!   Project Host, which the database cannot know about.
//!
//! Both are needed. The constraint alone would happily hand out a port that
//! another program on the host already owns; the bind test alone would race.

use std::collections::BTreeSet;
use std::net::TcpListener;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PortError {
    #[error("port {port} is outside the allowed range {start}–{end}")]
    OutOfRange { port: u16, start: u16, end: u16 },
    #[error("port {0} is privileged; ports below 1024 are not available")]
    Privileged(u16),
    #[error("port {0} is already in use")]
    InUse(u16),
    #[error("no free port remains in the range {start}–{end}")]
    PoolExhausted { start: u16, end: u16 },
}

/// The configured range projects are allocated from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortPool {
    pub start: u16,
    pub end: u16,
}

impl Default for PortPool {
    fn default() -> Self {
        Self {
            start: 20_000,
            end: 29_999,
        }
    }
}

impl PortPool {
    /// Lowest usable host port. Below this is privileged and never offered.
    pub const MINIMUM: u16 = 1024;

    pub fn new(start: u16, end: u16) -> Self {
        Self { start, end }
    }

    pub fn contains(&self, port: u16) -> bool {
        port >= self.start && port <= self.end
    }

    pub fn size(&self) -> u32 {
        if self.end < self.start {
            0
        } else {
            u32::from(self.end - self.start) + 1
        }
    }

    /// Validate a port the user asked for specifically.
    ///
    /// A request for 80 is refused rather than quietly raised to 1024: the two
    /// have different intent, and silently changing it would surprise them.
    pub fn validate_requested(&self, port: u16) -> Result<(), PortError> {
        if port < Self::MINIMUM {
            return Err(PortError::Privileged(port));
        }
        if !self.contains(port) {
            return Err(PortError::OutOfRange {
                port,
                start: self.start,
                end: self.end,
            });
        }
        Ok(())
    }

    /// First port in the pool that is neither already allocated nor bound by
    /// something else on the host.
    ///
    /// `taken` comes from the database; the bind probe covers everything else.
    pub fn allocate(&self, taken: &BTreeSet<u16>) -> Result<u16, PortError> {
        for port in self.start..=self.end {
            if taken.contains(&port) {
                continue;
            }
            if is_available(port) {
                return Ok(port);
            }
        }
        Err(PortError::PoolExhausted {
            start: self.start,
            end: self.end,
        })
    }

    /// Allocate several distinct ports at once, for a project publishing more
    /// than one. Reserving as we go stops the same port being handed out twice
    /// within a single request.
    pub fn allocate_many(
        &self,
        count: usize,
        taken: &BTreeSet<u16>,
    ) -> Result<Vec<u16>, PortError> {
        let mut reserved = taken.clone();
        let mut allocated = Vec::with_capacity(count);
        for _ in 0..count {
            let port = self.allocate(&reserved)?;
            reserved.insert(port);
            allocated.push(port);
        }
        Ok(allocated)
    }
}

/// Whether the host will let us bind this port on loopback right now.
///
/// Binding and immediately dropping is the only reliable test — an OS-level
/// "is it free" query does not exist portably, and parsing `netstat` would be
/// both slow and wrong.
pub fn is_available(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_pool_matches_the_documented_range() {
        let pool = PortPool::default();
        assert_eq!(pool.start, 20_000);
        assert_eq!(pool.end, 29_999);
        assert_eq!(pool.size(), 10_000);
    }

    #[test]
    fn privileged_ports_are_refused_rather_than_raised() {
        let pool = PortPool::default();
        for port in [0, 22, 80, 443, 1023] {
            assert!(
                matches!(pool.validate_requested(port), Err(PortError::Privileged(_))),
                "port {port} should be refused as privileged"
            );
        }
    }

    #[test]
    fn ports_outside_the_pool_are_refused() {
        let pool = PortPool::default();
        assert!(matches!(
            pool.validate_requested(8080),
            Err(PortError::OutOfRange { .. })
        ));
        assert!(pool.validate_requested(20_000).is_ok());
        assert!(pool.validate_requested(29_999).is_ok());
    }

    #[test]
    fn allocation_skips_ports_the_database_already_holds() {
        let pool = PortPool::new(20_000, 20_010);
        let taken: BTreeSet<u16> = (20_000..20_005).collect();
        let allocated = pool.allocate(&taken).expect("allocate");
        assert!(allocated >= 20_005, "got {allocated}");
        assert!(!taken.contains(&allocated));
    }

    #[test]
    fn allocation_skips_a_port_held_by_another_process() {
        // The case the database cannot know about.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let occupied = listener.local_addr().expect("addr").port();

        let pool = PortPool::new(occupied, occupied.saturating_add(5));
        let allocated = pool.allocate(&BTreeSet::new()).expect("allocate");
        assert_ne!(allocated, occupied, "must not hand out an occupied port");

        drop(listener);
    }

    #[test]
    fn an_exhausted_pool_is_an_error_not_a_wrong_answer() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let only = listener.local_addr().expect("addr").port();

        let pool = PortPool::new(only, only);
        assert!(matches!(
            pool.allocate(&BTreeSet::new()),
            Err(PortError::PoolExhausted { .. })
        ));

        drop(listener);
    }

    #[test]
    fn a_fully_reserved_pool_is_exhausted() {
        let pool = PortPool::new(20_000, 20_002);
        let taken: BTreeSet<u16> = (20_000..=20_002).collect();
        assert!(matches!(
            pool.allocate(&taken),
            Err(PortError::PoolExhausted { .. })
        ));
    }

    #[test]
    fn multiple_allocations_are_distinct() {
        let pool = PortPool::new(20_000, 20_050);
        let ports = pool.allocate_many(5, &BTreeSet::new()).expect("allocate");
        assert_eq!(ports.len(), 5);
        let unique: BTreeSet<u16> = ports.iter().copied().collect();
        assert_eq!(unique.len(), 5, "allocated a duplicate: {ports:?}");
    }

    #[test]
    fn an_inverted_pool_has_no_capacity() {
        let pool = PortPool::new(30_000, 20_000);
        assert_eq!(pool.size(), 0);
        assert!(!pool.contains(25_000));
    }

    #[test]
    fn containment_is_inclusive_at_both_ends() {
        let pool = PortPool::new(20_000, 20_010);
        assert!(pool.contains(20_000));
        assert!(pool.contains(20_010));
        assert!(!pool.contains(19_999));
        assert!(!pool.contains(20_011));
    }
}

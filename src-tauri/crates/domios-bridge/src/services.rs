//! The messaging services this build knows about.
//!
//! A service id here is the same value `codeterm bridge send --service <id>`
//! takes and the same value `MessagingService::id` reports, so every layer that
//! names a service — the send gate, the push-channel registry, the settings
//! grid — resolves it from this one table instead of restating a list.

/// One service the bridge concept covers.
pub struct ServiceDescriptor {
    /// Stable wire id. Never rendered to the user.
    pub id: &'static str,
    /// User-facing name for a column or a control.
    pub label: &'static str,
    /// Whether this build can actually carry an outbound message here. A known
    /// service with no transport is honest about it rather than absent.
    pub outbound: bool,
}

/// The roster. Adding a service is a row here plus that service's own module —
/// nothing downstream enumerates services by hand.
pub const SERVICES: &[ServiceDescriptor] = &[
    ServiceDescriptor {
        id: "telegram",
        label: "Telegram",
        outbound: true,
    },
    ServiceDescriptor {
        id: "discord",
        label: "Discord",
        outbound: false,
    },
];

/// Resolve a service id. An id outside the roster is `None`, never a
/// pass-through string some later layer would treat as real.
pub fn find(id: &str) -> Option<&'static ServiceDescriptor> {
    let id = id.trim();
    SERVICES.iter().find(|service| service.id == id)
}

pub fn ids() -> Vec<&'static str> {
    SERVICES.iter().map(|service| service.id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_ids_are_distinct_and_resolvable() {
        let distinct: std::collections::HashSet<&str> = ids().into_iter().collect();
        assert_eq!(distinct.len(), SERVICES.len(), "a service id collides");
        for service in SERVICES {
            assert_eq!(find(service.id).map(|found| found.id), Some(service.id));
            assert!(
                !service.label.trim().is_empty(),
                "{} needs copy",
                service.id
            );
        }
        assert_eq!(find("  telegram  ").map(|s| s.id), Some("telegram"));
        assert!(find("a-service-nobody-registered").is_none());
    }

    #[test]
    fn at_least_one_service_carries_outbound_traffic() {
        assert!(
            SERVICES.iter().any(|service| service.outbound),
            "a roster where nothing can send is a roster over nothing"
        );
    }
}

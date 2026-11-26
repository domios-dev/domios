use std::fmt;
use std::net::{IpAddr, SocketAddrV4};

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

pub const CLOUD_INGRESS_PORT: u16 = 7687;
pub const PROVIDER_PROXY_PORT: u16 = 443;
pub const MAX_PROVIDER_PROXY_HOST_BYTES: usize = 253;

pub fn direct_address_allowed(address: SocketAddrV4) -> bool {
    address.port() == CLOUD_INGRESS_PORT && crate::network_host::public_ip((*address.ip()).into())
}

pub fn provider_proxy_host_allowed(host: &str) -> bool {
    if host.is_empty()
        || host.len() > MAX_PROVIDER_PROXY_HOST_BYTES
        || host.parse::<IpAddr>().is_ok()
        || host == "localhost"
        || host.ends_with(".localhost")
    {
        return false;
    }
    if !host
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
    {
        return false;
    }
    let mut label_count = 0usize;
    let mut last_label = "";
    for label in host.split('.') {
        if label.is_empty() || label.len() > 63 {
            return false;
        }
        let bytes = label.as_bytes();
        let valid_edge = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
        if !valid_edge(bytes[0]) || !valid_edge(bytes[bytes.len() - 1]) {
            return false;
        }
        label_count += 1;
        last_label = label;
    }
    // RFC 1123 / ICANN TLD rules: at least two labels, alphabetic start on TLD blocks IPv4 lookalikes.
    label_count >= 2
        && last_label.len() >= 2
        && last_label.as_bytes()[0].is_ascii_lowercase()
        && !last_label.bytes().all(|b| b.is_ascii_digit())
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudEndpoint {
    Direct(SocketAddrV4),
    ProviderProxy { host: String },
}

impl CloudEndpoint {
    pub fn is_valid(&self) -> bool {
        match self {
            Self::Direct(address) => direct_address_allowed(*address),
            Self::ProviderProxy { host } => provider_proxy_host_allowed(host),
        }
    }

    pub fn direct_addr(&self) -> Option<SocketAddrV4> {
        match self {
            Self::Direct(address) => Some(*address),
            Self::ProviderProxy { .. } => None,
        }
    }

    pub fn provider_proxy_host(&self) -> Option<&str> {
        match self {
            Self::Direct(_) => None,
            Self::ProviderProxy { host } => Some(host.as_str()),
        }
    }
}

impl fmt::Display for CloudEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Direct(address) => write!(f, "{address}"),
            Self::ProviderProxy { host } => write!(f, "wss://{host}"),
        }
    }
}

pub fn deserialize_optional_cloud_endpoint<'de, D>(
    deserializer: D,
) -> Result<Option<CloudEndpoint>, D::Error>
where
    D: Deserializer<'de>,
{
    struct OptionalEndpointVisitor;

    impl<'de> Visitor<'de> for OptionalEndpointVisitor {
        type Value = Option<CloudEndpoint>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("null, a legacy SocketAddrV4 string, or a CloudEndpoint object")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D2>(self, deserializer: D2) -> Result<Self::Value, D2::Error>
        where
            D2: Deserializer<'de>,
        {
            deserializer.deserialize_any(self)
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let addr = v
                .parse::<SocketAddrV4>()
                .map_err(|_| E::custom(format!("invalid legacy direct endpoint address: {v}")))?;
            Ok(Some(CloudEndpoint::Direct(addr)))
        }

        fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
        where
            A: de::MapAccess<'de>,
        {
            CloudEndpoint::deserialize(de::value::MapAccessDeserializer::new(map)).map(Some)
        }
    }

    deserializer.deserialize_option(OptionalEndpointVisitor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn provider_proxy_host_accepts_valid_lowercase_dns_names() {
        for host in [
            "domios-a1b2c3d4.fly.dev",
            "a.co",
            "test-cloud.example.test",
            "sub1.sub-2.example.dev",
            "123.fly.dev",
        ] {
            assert!(provider_proxy_host_allowed(host), "expected valid: {host}");
        }
        let max_host = format!(
            "{}.{}.{}.{}",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(61)
        );
        assert_eq!(max_host.len(), MAX_PROVIDER_PROXY_HOST_BYTES);
        assert!(provider_proxy_host_allowed(&max_host));
    }

    #[test]
    fn provider_proxy_host_refuses_ip_literals_ports_uppercase_and_oversized_names() {
        let too_long = format!(
            "{}.{}.{}.{}",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(62)
        );
        assert_eq!(too_long.len(), MAX_PROVIDER_PROXY_HOST_BYTES + 1);
        let long_label = format!("{}.fly.dev", "a".repeat(64));
        for bad in [
            "",
            "localhost",
            "sub.localhost",
            "127.0.0.1",
            "8.8.8.8",
            "198.51.100.10",
            "127.1",
            "0x7f.0.0.1",
            "::1",
            "[::1]",
            "App.Fly.Dev",
            "app.fly.dev:443",
            "app.fly.dev:8443",
            "wss://app.fly.dev",
            "app.fly.dev/path",
            "app.fly.dev.",
            ".app.fly.dev",
            "app..fly.dev",
            "-app.fly.dev",
            "app-.fly.dev",
            "app_1.fly.dev",
            "app.123",
            "app.x",
            too_long.as_str(),
            long_label.as_str(),
        ] {
            assert!(!provider_proxy_host_allowed(bad), "expected refused: {bad}");
        }
    }

    #[test]
    fn persisted_endpoint_deserializes_legacy_socket_addr_as_direct_and_round_trips_union() {
        #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
        struct Row {
            #[serde(
                default,
                skip_serializing_if = "Option::is_none",
                alias = "endpoint_addr",
                deserialize_with = "deserialize_optional_cloud_endpoint"
            )]
            endpoint: Option<CloudEndpoint>,
        }

        let legacy: Row =
            serde_json::from_str(r#"{"endpoint_addr":"198.51.100.10:7687"}"#).unwrap();
        assert_eq!(
            legacy.endpoint,
            Some(CloudEndpoint::Direct(SocketAddrV4::new(
                Ipv4Addr::new(198, 51, 100, 10),
                CLOUD_INGRESS_PORT
            )))
        );

        let absent: Row = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(absent.endpoint, None);

        let proxy = Row {
            endpoint: Some(CloudEndpoint::ProviderProxy {
                host: "domios-a1b2c3d4.fly.dev".into(),
            }),
        };
        let encoded = serde_json::to_string(&proxy).unwrap();
        assert_eq!(serde_json::from_str::<Row>(&encoded).unwrap(), proxy);
    }
}

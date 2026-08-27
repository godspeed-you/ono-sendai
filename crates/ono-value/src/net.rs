//! The `IpNetwork` semantic scalar of spec §10.2.

use std::fmt;
use std::net::IpAddr;

use ono_core::ErrorCode;

use crate::ErrorValue;

/// An IP address together with a prefix length, such as `192.0.2.0/24`.
///
/// ```
/// use ono_value::IpNetwork;
/// let network = IpNetwork::parse("2001:db8::/32")?;
/// assert_eq!(network.prefix_len(), 32);
/// assert_eq!(network.to_string(), "2001:db8::/32");
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IpNetwork {
    address: IpAddr,
    prefix_len: u8,
}

impl IpNetwork {
    /// Creates a network from an address and a prefix length.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the prefix length is longer than the address family
    /// allows.
    pub fn new(address: IpAddr, prefix_len: u8) -> Result<Self, ErrorValue> {
        let maximum = if address.is_ipv4() { 32 } else { 128 };
        if prefix_len > maximum {
            return Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("a /{prefix_len} prefix does not fit a {maximum}-bit address"),
            ));
        }
        Ok(Self {
            address,
            prefix_len,
        })
    }

    /// The network address.
    #[must_use]
    pub const fn address(self) -> IpAddr {
        self.address
    }

    /// The prefix length in bits.
    #[must_use]
    pub const fn prefix_len(self) -> u8 {
        self.prefix_len
    }

    /// Parses the canonical `address/prefix` form.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the text is not an address followed by a prefix
    /// length.
    pub fn parse(text: &str) -> Result<Self, ErrorValue> {
        let error = || {
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("`{text}` is not an IP network"),
            )
        };
        let (address, prefix) = text.rsplit_once('/').ok_or_else(error)?;
        let address: IpAddr = address.parse().map_err(|_| error())?;
        let prefix_len: u8 = prefix.parse().map_err(|_| error())?;
        Self::new(address, prefix_len)
    }
}

impl fmt::Display for IpNetwork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.address, self.prefix_len)
    }
}

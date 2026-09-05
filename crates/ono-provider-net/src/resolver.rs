//! The C library's resolver, behind safe functions.
//!
//! This is the one module in the crate that touches `unsafe`, and it exposes two calls: a name
//! to its addresses, and an address to its name. Everything a caller receives is an owned Rust
//! value; the C allocations are freed before either function returns.

use std::ffi::{CStr, CString};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ono_core::ErrorCode;
use ono_value::ErrorValue;

/// The record types the system resolver can answer with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordType {
    /// An IPv4 address for a name.
    A,
    /// An IPv6 address for a name.
    Aaaa,
    /// A name for an address.
    Ptr,
}

impl RecordType {
    /// The spelling `docs/contracts/schemas/dns-record.v1.yaml` uses.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            RecordType::A => "A",
            RecordType::Aaaa => "AAAA",
            RecordType::Ptr => "PTR",
        }
    }

    /// Parses the contract's spelling, case-insensitively.
    ///
    /// # Errors
    ///
    /// `type.mismatch` naming the three spellings when `text` is none of them.
    pub fn parse(text: &str) -> Result<Self, ErrorValue> {
        match text.to_ascii_uppercase().as_str() {
            "A" => Ok(RecordType::A),
            "AAAA" => Ok(RecordType::Aaaa),
            "PTR" => Ok(RecordType::Ptr),
            other => Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("`{other}` is not a record type the system resolver answers"),
            )
            .with_help("`--type` takes A, AAAA or PTR")),
        }
    }

    /// The type of an address record for `address`.
    #[must_use]
    pub const fn of(address: IpAddr) -> Self {
        match address {
            IpAddr::V4(_) => RecordType::A,
            IpAddr::V6(_) => RecordType::Aaaa,
        }
    }
}

/// Every address `name` resolves to, in the order the resolver returned them, without duplicates.
///
/// # Errors
///
/// `io.not_found` when the resolver knows no such name, `provider.unavailable` (retryable) when
/// it could not ask, `type.mismatch` when `name` cannot be passed to it at all.
pub(crate) fn addresses_of(name: &str) -> Result<Vec<IpAddr>, ErrorValue> {
    let host = CString::new(name).map_err(|_| {
        ErrorValue::new(
            ErrorCode::TypeMismatch,
            "a name to resolve cannot contain a NUL byte",
        )
    })?;
    // SAFETY: `addrinfo` is a plain C struct for which all-zero bytes are a valid value — the
    // documented way to build hints is to zero it and set the fields of interest.
    let mut hints: libc::addrinfo = unsafe { std::mem::zeroed() };
    hints.ai_family = libc::AF_UNSPEC;
    // One socket type, or every address comes back once per type the resolver knows.
    hints.ai_socktype = libc::SOCK_STREAM;
    let mut list: *mut libc::addrinfo = std::ptr::null_mut();

    // SAFETY: `host` is a NUL-terminated string that outlives the call, `hints` is fully
    // initialised, `list` is a valid out-pointer, and the list the call allocates is released
    // with `freeaddrinfo` exactly once, below, on every path after a successful return.
    let code = unsafe {
        libc::getaddrinfo(
            host.as_ptr(),
            std::ptr::null(),
            &raw const hints,
            &raw mut list,
        )
    };
    if code != 0 {
        return Err(lookup_error(code, name));
    }

    let mut addresses = Vec::new();
    let mut cursor = list;
    while !cursor.is_null() {
        // SAFETY: `cursor` is a node of the list `getaddrinfo` returned, which stays allocated
        // until `freeaddrinfo` below; the walk only reads it.
        let node = unsafe { &*cursor };
        if let Some(address) = address_of(node)
            && !addresses.contains(&address)
        {
            addresses.push(address);
        }
        cursor = node.ai_next;
    }
    // SAFETY: `list` came from the successful `getaddrinfo` above and is freed exactly here.
    unsafe { libc::freeaddrinfo(list) };
    Ok(addresses)
}

/// The address a resolver node carries, if it is one this crate can read.
fn address_of(node: &libc::addrinfo) -> Option<IpAddr> {
    if node.ai_addr.is_null() {
        return None;
    }
    let length = usize::try_from(node.ai_addrlen).ok()?;
    match node.ai_family {
        libc::AF_INET if length >= std::mem::size_of::<libc::sockaddr_in>() => {
            // SAFETY: the resolver reports `AF_INET` with a length that covers a `sockaddr_in`,
            // which is the type it stores behind `ai_addr` for that family; the read is
            // unaligned-safe and copies the struct out.
            let address =
                unsafe { std::ptr::read_unaligned(node.ai_addr.cast::<libc::sockaddr_in>()) };
            Some(IpAddr::V4(Ipv4Addr::from(u32::from_be(
                address.sin_addr.s_addr,
            ))))
        }
        libc::AF_INET6 if length >= std::mem::size_of::<libc::sockaddr_in6>() => {
            // SAFETY: as above, for `AF_INET6` and `sockaddr_in6`.
            let address =
                unsafe { std::ptr::read_unaligned(node.ai_addr.cast::<libc::sockaddr_in6>()) };
            Some(IpAddr::V6(Ipv6Addr::from(address.sin6_addr.s6_addr)))
        }
        _ => None,
    }
}

/// The name `address` maps to, as the resolver's reverse lookup answers it.
///
/// # Errors
///
/// `io.not_found` when no name maps to the address, `provider.unavailable` (retryable) when the
/// resolver could not ask.
pub(crate) fn name_of(address: IpAddr) -> Result<String, ErrorValue> {
    let mut host = [0 as libc::c_char; libc::NI_MAXHOST as usize];
    let code = match address {
        IpAddr::V4(v4) => {
            // SAFETY: all-zero bytes are a valid `sockaddr_in`; the fields that matter are set
            // explicitly below.
            let mut socket: libc::sockaddr_in = unsafe { std::mem::zeroed() };
            socket.sin_family = libc::sa_family_t::try_from(libc::AF_INET).unwrap_or(0);
            socket.sin_addr.s_addr = u32::from(v4).to_be();
            let length =
                libc::socklen_t::try_from(std::mem::size_of::<libc::sockaddr_in>()).unwrap_or(0);
            // SAFETY: `socket` is a fully initialised `sockaddr_in` whose length is passed
            // with it; `host` is a writable buffer whose length is passed with it; no service
            // buffer is requested (null pointer, zero length), which the call permits.
            unsafe {
                libc::getnameinfo(
                    (&raw const socket).cast::<libc::sockaddr>(),
                    length,
                    host.as_mut_ptr(),
                    libc::NI_MAXHOST,
                    std::ptr::null_mut(),
                    0,
                    libc::NI_NAMEREQD,
                )
            }
        }
        IpAddr::V6(v6) => {
            // SAFETY: all-zero bytes are a valid `sockaddr_in6`.
            let mut socket: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
            socket.sin6_family = libc::sa_family_t::try_from(libc::AF_INET6).unwrap_or(0);
            socket.sin6_addr.s6_addr = v6.octets();
            let length =
                libc::socklen_t::try_from(std::mem::size_of::<libc::sockaddr_in6>()).unwrap_or(0);
            // SAFETY: as for IPv4, with a `sockaddr_in6`.
            unsafe {
                libc::getnameinfo(
                    (&raw const socket).cast::<libc::sockaddr>(),
                    length,
                    host.as_mut_ptr(),
                    libc::NI_MAXHOST,
                    std::ptr::null_mut(),
                    0,
                    libc::NI_NAMEREQD,
                )
            }
        }
    };
    if code != 0 {
        return Err(lookup_error(code, &address.to_string()));
    }
    // SAFETY: a successful `getnameinfo` wrote a NUL-terminated string into `host`, whose
    // length bounds the read.
    let name = unsafe { CStr::from_ptr(host.as_ptr()) };
    Ok(name.to_string_lossy().into_owned())
}

/// The structured form of a resolver failure (spec §43).
///
/// "No such name" is `io.not_found`; a resolver that could not be asked — no network, no
/// nameserver answering, a failing NSS backend — is `provider.unavailable` and retryable,
/// because the answer may exist and nothing here has learned otherwise.
///
/// ```
/// use ono_provider_net::lookup_error;
/// let error = lookup_error(libc::EAI_NONAME, "nowhere.invalid");
/// assert_eq!(error.code().code(), "Ono-Sendai-E0301");
/// let error = lookup_error(libc::EAI_AGAIN, "example.com");
/// assert_eq!(error.code().code(), "Ono-Sendai-E0401");
/// assert_eq!(error.retryable(), Some(true));
/// ```
#[must_use]
pub fn lookup_error(code: libc::c_int, query: &str) -> ErrorValue {
    // SAFETY: `gai_strerror` returns a pointer to a static, NUL-terminated string for every
    // code, including ones it does not know.
    let reason = unsafe { CStr::from_ptr(libc::gai_strerror(code)) }
        .to_string_lossy()
        .into_owned();
    match code {
        libc::EAI_NONAME | libc::EAI_NODATA | libc::EAI_SERVICE => ErrorValue::new(
            ErrorCode::IoNotFound,
            format!("the resolver knows no `{query}`: {reason}"),
        )
        .with_help("check the spelling; a name that exists only in DNS needs a reachable resolver"),
        libc::EAI_SYSTEM => {
            let errno = std::io::Error::last_os_error();
            ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                format!("the resolver could not look up `{query}`: {errno}"),
            )
            .with_retryable(true)
        }
        _ => ErrorValue::new(
            ErrorCode::ProviderUnavailable,
            format!("the resolver could not answer for `{query}`: {reason}"),
        )
        .with_help(
            "no nameserver answered, or NSS refused; nothing is being hidden by an empty answer",
        )
        .with_retryable(true),
    }
}

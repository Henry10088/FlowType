use std::net::{IpAddr, Ipv4Addr};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST, GetAdaptersAddresses,
    IP_ADAPTER_ADDRESSES_LH,
};
use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows_sys::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};

pub fn preferred_ipv4() -> IpAddr {
    candidate_ipv4s()
        .into_iter()
        .next()
        .map(IpAddr::V4)
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST))
}

pub fn candidate_ipv4s() -> Vec<Ipv4Addr> {
    let mut candidates = adapter_ipv4s()
        .into_iter()
        .filter(|(name, address)| address_score(name, *address) >= 100)
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(name, address)| std::cmp::Reverse(address_score(name, *address)));
    candidates.dedup_by_key(|(_, address)| *address);
    candidates
        .into_iter()
        .map(|(_, address)| address)
        .take(6)
        .collect()
}

fn adapter_ipv4s() -> Vec<(String, Ipv4Addr)> {
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut byte_count = 0_u32;
    let first =
        unsafe { GetAdaptersAddresses(AF_INET as u32, flags, null(), null_mut(), &mut byte_count) };
    if first != ERROR_BUFFER_OVERFLOW || byte_count == 0 {
        return Vec::new();
    }

    let word_count = (byte_count as usize).div_ceil(size_of::<u64>());
    let mut buffer = vec![0_u64; word_count];
    let first_adapter = buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    if unsafe {
        GetAdaptersAddresses(
            AF_INET as u32,
            flags,
            null(),
            first_adapter,
            &mut byte_count,
        )
    } != NO_ERROR
    {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut adapter = first_adapter;
    while !adapter.is_null() {
        let current = unsafe { &*adapter };
        if current.OperStatus == IfOperStatusUp {
            let name = wide_ptr_to_string(current.FriendlyName);
            let mut unicast = current.FirstUnicastAddress;
            while !unicast.is_null() {
                let socket = unsafe { (*unicast).Address.lpSockaddr };
                if !socket.is_null() && unsafe { (*socket).sa_family } == AF_INET {
                    let address = unsafe { (*(socket.cast::<SOCKADDR_IN>())).sin_addr.S_un.S_un_b };
                    let address =
                        Ipv4Addr::new(address.s_b1, address.s_b2, address.s_b3, address.s_b4);
                    if !address.is_loopback()
                        && !address.is_unspecified()
                        && !address.is_link_local()
                    {
                        result.push((name.clone(), address));
                    }
                }
                unicast = unsafe { (*unicast).Next };
            }
        }
        adapter = current.Next;
    }
    result
}

fn address_score(adapter_name: &str, address: Ipv4Addr) -> i32 {
    let name = adapter_name.to_ascii_lowercase();
    if name.contains("tailscale") {
        return 1_000;
    }
    if name.contains("vEthernet")
        || name.contains("vethernet")
        || name.contains("hyper-v")
        || name.contains("wsl")
        || name.contains("default switch")
    {
        return 10;
    }
    if address.is_private() { 200 } else { 100 }
}

fn wide_ptr_to_string(pointer: *const u16) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let mut length = 0;
    while unsafe { *pointer.add(length) } != 0 {
        length += 1;
    }
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(pointer, length) })
}

use std::mem::size_of;

#[cfg(test)]
mod tests {
    use super::address_score;
    use std::net::Ipv4Addr;

    #[test]
    fn prefers_tailscale_over_hyper_v_and_private_lan() {
        let tailscale = address_score("Tailscale", Ipv4Addr::new(100, 0, 0, 13));
        let lan = address_score("Wi-Fi", Ipv4Addr::new(192, 168, 1, 20));
        let hyper_v = address_score(
            "vEthernet (Default Switch)",
            Ipv4Addr::new(192, 168, 77, 161),
        );
        assert!(tailscale > lan);
        assert!(lan > hyper_v);
    }

    #[test]
    fn excludes_known_virtual_adapters_from_pairing_candidates() {
        assert!(
            address_score(
                "vEthernet (Default Switch)",
                Ipv4Addr::new(192, 168, 77, 161)
            ) < 100
        );
        assert!(address_score("Wi-Fi", Ipv4Addr::new(192, 168, 1, 20)) >= 100);
    }
}

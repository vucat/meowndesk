//! Kernel flags, ifindex + MAC identity, and native interface byte counters. No name heuristics.
use crate::telemetry::providers::{
    ProviderError, linux::system::read_u64, network::InterfaceCounters,
};
use std::fs;
pub fn interfaces() -> Result<Vec<InterfaceCounters>, ProviderError> {
    let mut result = Vec::new();
    for entry in fs::read_dir("/sys/class/net").map_err(ProviderError::from)? {
        let path = entry.map_err(ProviderError::from)?.path();
        let flags = fs::read_to_string(path.join("flags")).map_err(ProviderError::from)?;
        let flags = u32::from_str_radix(flags.trim().trim_start_matches("0x"), 16)
            .map_err(|_| ProviderError::Invalid)?;
        if flags & libc::IFF_UP as u32 == 0 || flags & libc::IFF_LOOPBACK as u32 != 0 {
            continue;
        }
        let index = read_u64(path.join("ifindex"))?;
        let mac = fs::read_to_string(path.join("address")).map_err(ProviderError::from)?;
        result.push(InterfaceCounters {
            id: format!("linux-if:{index}:{}", mac.trim()),
            up: true,
            loopback: false,
            received: read_u64(path.join("statistics/rx_bytes"))?,
            sent: read_u64(path.join("statistics/tx_bytes"))?,
        });
    }
    Ok(result)
}

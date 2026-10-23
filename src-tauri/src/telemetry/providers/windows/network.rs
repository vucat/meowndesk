//! Windows interface GUIDs are stable identity; native status/type flags select traffic scope.
use crate::telemetry::providers::{ProviderError, network::InterfaceCounters};
use windows::Win32::NetworkManagement::{IpHelper::*, Ndis::IfOperStatusUp};
pub fn interfaces() -> Result<Vec<InterfaceCounters>, ProviderError> {
    let mut table = std::ptr::null_mut();
    let status = unsafe { GetIfTable2(&mut table) };
    if status.0 != 0 {
        return Err(if status.0 == 5 {
            ProviderError::PermissionDenied
        } else {
            ProviderError::Unavailable
        });
    }
    if table.is_null() {
        return Err(ProviderError::Unavailable);
    }
    struct Table(*mut MIB_IF_TABLE2);
    impl Drop for Table {
        fn drop(&mut self) {
            unsafe {
                FreeMibTable(self.0.cast());
            }
        }
    }
    let table = Table(table);
    // SAFETY: GetIfTable2 allocates NumEntries contiguous rows; guard frees after copying.
    let rows = unsafe {
        std::slice::from_raw_parts((*table.0).Table.as_ptr(), (*table.0).NumEntries as usize)
    };
    Ok(rows
        .iter()
        .filter(|r| r.OperStatus == IfOperStatusUp && r.Type != IF_TYPE_SOFTWARE_LOOPBACK)
        .map(|r| InterfaceCounters {
            id: format!("windows-if:{:?}", r.InterfaceGuid),
            up: true,
            loopback: false,
            received: r.InOctets,
            sent: r.OutOctets,
        })
        .collect())
}

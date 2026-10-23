//! RAII PDH queries. Handles never escape; array storage is aligned and bounded before allocation.
use crate::telemetry::providers::ProviderError;
use windows::{Win32::System::Performance::*, core::PCWSTR};
pub(crate) fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
pub(crate) fn text(value: &[u16]) -> String {
    String::from_utf16_lossy(&value[..value.iter().position(|v| *v == 0).unwrap_or(value.len())])
}
pub(crate) fn status(code: u32) -> Result<(), ProviderError> {
    match code {
        0 => Ok(()),
        5 => Err(ProviderError::PermissionDenied),
        _ => Err(ProviderError::Unavailable),
    }
}
pub(crate) struct Query {
    query: PDH_HQUERY,
    counters: Vec<PDH_HCOUNTER>,
}
// SAFETY: PDH query handles have no thread affinity. This owner is accessed exclusively through
// a provider Mutex; neither raw handles nor borrowed PDH buffers are exposed to other threads.
unsafe impl Send for Query {}
impl Query {
    pub fn new(paths: &[String]) -> Result<Self, ProviderError> {
        let mut query = PDH_HQUERY::default();
        unsafe {
            status(PdhOpenQueryW(PCWSTR::null(), 0, &mut query))?;
        }
        let mut owner = Self {
            query,
            counters: Vec::new(),
        };
        for path in paths {
            let path = wide(path);
            let mut counter = PDH_HCOUNTER::default();
            unsafe {
                status(PdhAddEnglishCounterW(
                    owner.query,
                    PCWSTR(path.as_ptr()),
                    0,
                    &mut counter,
                ))?;
            }
            owner.counters.push(counter);
        }
        Ok(owner)
    }
    pub fn collect(&mut self) -> Result<(), ProviderError> {
        unsafe { status(PdhCollectQueryData(self.query)) }
    }
    pub fn raw(&self, index: usize) -> Result<u64, ProviderError> {
        let mut value = PDH_RAW_COUNTER::default();
        unsafe {
            status(PdhGetRawCounterValue(
                *self.counters.get(index).ok_or(ProviderError::Invalid)?,
                None,
                &mut value,
            ))?;
        }
        if value.CStatus > 1 {
            return Err(ProviderError::Unavailable);
        }
        value
            .FirstValue
            .try_into()
            .map_err(|_| ProviderError::Invalid)
    }
    pub fn array(&self, index: usize) -> Result<Vec<(String, f64)>, ProviderError> {
        let counter = *self.counters.get(index).ok_or(ProviderError::Invalid)?;
        let mut bytes = 0;
        let mut count = 0;
        // PDH_FMT_NOCAP100 (0x8000) avoids hiding invalid sums by capping percentages.
        let format = PDH_FMT(PDH_FMT_DOUBLE.0 | 0x8000);
        let result =
            unsafe { PdhGetFormattedCounterArrayW(counter, format, &mut bytes, &mut count, None) };
        if result != PDH_MORE_DATA {
            return Err(ProviderError::Unavailable);
        }
        if bytes == 0 || bytes > 16 * 1024 * 1024 {
            return Err(ProviderError::Invalid);
        }
        let mut buffer = vec![0u64; (bytes as usize).div_ceil(8)];
        unsafe {
            status(PdhGetFormattedCounterArrayW(
                counter,
                format,
                &mut bytes,
                &mut count,
                Some(buffer.as_mut_ptr().cast()),
            ))?;
            if count as usize
                > buffer.len() * 8 / std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>()
            {
                return Err(ProviderError::Invalid);
            }
            let items = std::slice::from_raw_parts(
                buffer.as_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>(),
                count as usize,
            );
            let mut values = Vec::new();
            for item in items {
                if item.FmtValue.CStatus > 1 {
                    return Err(ProviderError::Unavailable);
                }
                values.push((
                    item.szName
                        .to_string()
                        .map_err(|_| ProviderError::Invalid)?,
                    item.FmtValue.Anonymous.doubleValue,
                ));
            }
            Ok(values)
        }
    }
}
impl Drop for Query {
    fn drop(&mut self) {
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}

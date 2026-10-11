//! Memory figures that match what users see elsewhere.

/// macOS: `phys_footprint`, the number Activity Monitor shows as "Memory".
/// RSS overcounts shared/compressed pages. Returns `None` when the kernel refuses
/// (another user's process) or on other platforms, so callers fall back to RSS.
#[cfg(target_os = "macos")]
pub fn footprint(pid: u32) -> Option<u64> {
    let pid = libc::c_int::try_from(pid).ok()?;
    // SAFETY: `rusage_info_v2` is plain old data; zeroed is a valid value.
    let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
    // SAFETY: the buffer is a valid, writable `rusage_info_v2`, matching the flavor.
    let ret = unsafe {
        libc::proc_pid_rusage(
            pid,
            libc::RUSAGE_INFO_V2,
            (&raw mut info).cast::<libc::rusage_info_t>(),
        )
    };
    (ret == 0).then_some(info.ri_phys_footprint)
}

#[cfg(not(target_os = "macos"))]
pub fn footprint(_pid: u32) -> Option<u64> {
    None
}

/// `512K`, `48.2M`, `1.3G`.
pub fn format_bytes(bytes: u64) -> String {
    const K: f64 = 1024.0;
    #[allow(clippy::cast_precision_loss)]
    let b = bytes as f64;
    if b < K * K {
        format!("{:.0}K", b / K)
    } else if b < K * K * K {
        format!("{:.1}M", b / (K * K))
    } else {
        format!("{:.1}G", b / (K * K * K))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bytes() {
        assert_eq!(format_bytes(0), "0K");
        assert_eq!(format_bytes(512 * 1024), "512K");
        assert_eq!(format_bytes(48 * 1024 * 1024 + 200 * 1024), "48.2M");
        assert_eq!(format_bytes(1_395_864_371), "1.3G");
    }

    #[test]
    fn own_footprint_is_readable_on_macos() {
        let own = footprint(std::process::id());
        if cfg!(target_os = "macos") {
            assert!(own.is_some_and(|b| b > 0));
        } else {
            assert_eq!(own, None);
        }
    }
}

//! Sampling the host values every result records beside its measurement.

/// The host values sampled when a measurement starts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Sample {
    pub load1: Option<f64>,
    pub cpus: u32,
    pub mem_available_mib: Option<u64>,
}

pub(crate) fn sample() -> Sample {
    Sample {
        load1: load1(),
        cpus: std::thread::available_parallelism()
            .ok()
            .and_then(|count| u32::try_from(count.get()).ok())
            .unwrap_or(1),
        mem_available_mib: mem_available_mib(),
    }
}

#[cfg(unix)]
fn load1() -> Option<f64> {
    let mut averages = [0.0_f64; 3];
    // SAFETY: getloadavg writes at most `nelem` doubles into the buffer it is given, and
    // the buffer holds three; asking for one keeps well inside it.
    let written = unsafe { libc::getloadavg(averages.as_mut_ptr(), 1) };
    (written >= 1 && averages[0].is_finite()).then_some(averages[0])
}

#[cfg(not(unix))]
fn load1() -> Option<f64> {
    None
}

#[cfg(target_os = "linux")]
fn mem_available_mib() -> Option<u64> {
    parse_mem_available(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

#[cfg(not(target_os = "linux"))]
fn mem_available_mib() -> Option<u64> {
    None
}

/// The `MemAvailable` line of `/proc/meminfo`, in MiB.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_mem_available(meminfo: &str) -> Option<u64> {
    let line = meminfo
        .lines()
        .find(|line| line.starts_with("MemAvailable:"))?;
    let mut fields = line.split_whitespace().skip(1);
    let amount: u64 = fields.next()?.parse().ok()?;
    match fields.next() {
        Some("kB") => Some(amount / 1024),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_mem_available;

    #[test]
    fn mem_available_is_read_in_mib() {
        let meminfo = "MemTotal:       16384000 kB\nMemAvailable:    5242880 kB\n";
        assert_eq!(parse_mem_available(meminfo), Some(5120));
    }

    #[test]
    fn mem_available_absent_or_unitless_is_unreadable() {
        assert_eq!(parse_mem_available("MemTotal: 1 kB\n"), None);
        assert_eq!(parse_mem_available("MemAvailable: 1024\n"), None);
        assert_eq!(parse_mem_available("MemAvailable: lots kB\n"), None);
    }
}

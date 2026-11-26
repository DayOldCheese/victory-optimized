//! What the machine is doing, from Windows' own counters: CPU (system
//! times), RAM (memory status), GPU engine use and video memory in use
//! (performance counters, the same ones Task Manager shows), video memory
//! size (DXGI). A figure Windows does not give is `None`, never a guess.

#[derive(Debug, Clone, Default)]
pub struct Sample {
    pub cpu_percent: Option<f64>,
    pub ram_used_gb: Option<f64>,
    pub ram_total_gb: Option<f64>,
    /// The busiest 3D engine's use summed over processes, as Task Manager.
    pub gpu_percent: Option<f64>,
    pub vram_used_gb: Option<f64>,
    pub vram_total_gb: Option<f64>,
    pub gpu_name: Option<String>,
    /// Private bytes of the running `victory.exe`.
    pub game_ram_gb: Option<f64>,
}

#[cfg(windows)]
mod imp {
    use super::Sample;
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Performance::*;
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::GetSystemTimes;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    fn ft(t: FILETIME) -> u64 {
        u64::from(t.dwHighDateTime) << 32 | u64::from(t.dwLowDateTime)
    }

    pub struct Sampler {
        last_cpu: Option<(u64, u64)>,
        query: Option<PDH_HQUERY>,
        gpu: Option<PDH_HCOUNTER>,
        vram: Option<PDH_HCOUNTER>,
        game: Option<PDH_HCOUNTER>,
        vram_total: Option<(f64, String)>,
    }

    // SAFETY: PDH handles are used from the GUI thread only.
    unsafe impl Send for Sampler {}

    impl Sampler {
        pub fn new() -> Sampler {
            let mut s = Sampler { last_cpu: None, query: None, gpu: None, vram: None, game: None, vram_total: adapter() };
            let mut q: PDH_HQUERY = std::ptr::null_mut();
            // SAFETY: out pointer of ours; a null source is the live machine.
            if unsafe { PdhOpenQueryW(std::ptr::null(), 0, &mut q) } == 0 {
                let add = |path: &str| {
                    let mut c: PDH_HCOUNTER = std::ptr::null_mut();
                    let w = wide(path);
                    // SAFETY: a live query, a terminated path, our out pointer.
                    (unsafe { PdhAddEnglishCounterW(q, w.as_ptr(), 0, &mut c) } == 0).then_some(c)
                };
                s.gpu = add("\\GPU Engine(*engtype_3D)\\Utilization Percentage");
                s.vram = add("\\GPU Adapter Memory(*)\\Dedicated Usage");
                s.game = add("\\Process(victory)\\Private Bytes");
                s.query = Some(q);
                // SAFETY: a live query; the first collection primes rates.
                unsafe { PdhCollectQueryData(q) };
            }
            s
        }

        /// The sum of a wildcard counter's instances.
        fn sum(counter: Option<PDH_HCOUNTER>) -> Option<f64> {
            let c = counter?;
            let (mut bytes, mut items) = (0u32, 0u32);
            // SAFETY: a size query first (null buffer), then a buffer of that size.
            unsafe {
                let r = PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut bytes, &mut items, std::ptr::null_mut());
                if r != PDH_MORE_DATA || bytes == 0 {
                    return None;
                }
                let mut buf = vec![0u8; bytes as usize];
                let items_ptr = buf.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
                if PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut bytes, &mut items, items_ptr) != 0 {
                    return None;
                }
                let list = std::slice::from_raw_parts(items_ptr, items as usize);
                Some(list.iter().filter(|i| i.FmtValue.CStatus == 0).map(|i| i.FmtValue.Anonymous.doubleValue).sum())
            }
        }

        pub fn sample(&mut self) -> Sample {
            let mut s = Sample::default();
            // CPU: busy share of the time since the last sample.
            let (mut idle, mut kernel, mut user) = (FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 }, FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 }, FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 });
            // SAFETY: out pointers of ours.
            if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } != 0 {
                let (idle, total) = (ft(idle), ft(kernel) + ft(user));
                if let Some((li, lt)) = self.last_cpu {
                    let dt = total.saturating_sub(lt);
                    if dt > 0 {
                        s.cpu_percent = Some(100.0 * (1.0 - idle.saturating_sub(li) as f64 / dt as f64));
                    }
                }
                self.last_cpu = Some((idle, total));
            }
            // SAFETY: plain data, its size set.
            let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
            m.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
            // SAFETY: an initialised struct of the right size.
            if unsafe { GlobalMemoryStatusEx(&mut m) } != 0 {
                let gb = |b: u64| b as f64 / (1u64 << 30) as f64;
                s.ram_total_gb = Some(gb(m.ullTotalPhys));
                s.ram_used_gb = Some(gb(m.ullTotalPhys - m.ullAvailPhys));
            }
            if let Some(q) = self.query {
                // SAFETY: a live query.
                if unsafe { PdhCollectQueryData(q) } == 0 {
                    s.gpu_percent = Self::sum(self.gpu).map(|v| v.min(100.0));
                    s.vram_used_gb = Self::sum(self.vram).map(|b| b / (1u64 << 30) as f64);
                    s.game_ram_gb = Self::sum(self.game).map(|b| b / (1u64 << 30) as f64).filter(|&v| v > 0.0);
                }
            }
            if let Some((total, name)) = &self.vram_total {
                s.vram_total_gb = Some(*total);
                s.gpu_name = Some(name.clone());
            }
            s
        }
    }

    impl Drop for Sampler {
        fn drop(&mut self) {
            if let Some(q) = self.query {
                // SAFETY: our query.
                unsafe { PdhCloseQuery(q) };
            }
        }
    }

    /// The first hardware adapter's dedicated memory and name.
    fn adapter() -> Option<(f64, String)> {
        use windows::Win32::Graphics::Dxgi::*;
        // SAFETY: plain DXGI queries.
        unsafe {
            let f: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
            let mut i = 0;
            while let Ok(a) = f.EnumAdapters1(i) {
                let d = a.GetDesc1().ok()?;
                if d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0 {
                    let end = d.Description.iter().position(|&c| c == 0).unwrap_or(d.Description.len());
                    return Some((d.DedicatedVideoMemory as f64 / (1u64 << 30) as f64, String::from_utf16_lossy(&d.Description[..end])));
                }
                i += 1;
            }
            None
        }
    }
}

#[cfg(windows)]
pub use imp::Sampler;

#[cfg(not(windows))]
pub struct Sampler;
#[cfg(not(windows))]
impl Sampler {
    pub fn new() -> Sampler {
        Sampler
    }
    pub fn sample(&mut self) -> Sample {
        Sample::default()
    }
}

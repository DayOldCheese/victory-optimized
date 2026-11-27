//! NVIDIA DLSS Super Resolution in place of the title's FidelityFX upscale
//! (`VICTORY_UPSCALE=dlss`, a build with the `dlss` feature and the DLSS SDK).
//!
//! The title renders below its output size and asks FidelityFX to make the
//! output from the colour, depth and motion vectors it rendered, with the
//! camera's jitter: what DLSS takes too. The console's FidelityFX passes run
//! translated on the host, slowly on 32-lane cards (about 150 ms of card time
//! a frame on an RTX 3070 Ti, aleksesipenko/VIctory-RTX); DLSS runs on the
//! card's tensor cores in a millisecond or two. `fidelityfx` serves the
//! title's FidelityFX calls and hands each upscale here with host textures in
//! the states DLSS reads them in.
//!
//! After aleksesipenko/VIctory-RTX's `dlss.rs` (GPL-3.0): the same NGX calls.
//! Here the feature's flags come from the title's own FidelityFX creation
//! flags (HDR, depth inverted, display-size motion vectors, jitter in the
//! motion vectors, auto exposure), not from guesses, and the feature is
//! released only after the card has run every list that used it.
//!
//! The SDK is NVIDIA's (NVIDIA RTX SDKs licence) and is not in this
//! repository; this file holds only the declarations of the calls made. A
//! build that links it is for the machine it was made on.
//!
//! Switches: `VICTORY_DLSS_DIR` (the folder of `nvngx_dlss.dll`, else beside
//! the executable), `VICTORY_DLSS_QUALITY` (0 performance, 1 balanced, 2
//! quality, 3 ultra performance, 5 DLAA; else from the render/output ratio),
//! `VICTORY_DLSS_PRESET` (`J`, `K`, `L`, `M` or a number; else the driver's
//! default), `VICTORY_DLSS_MV_SIGN=-1` and `VICTORY_DLSS_JITTER_SIGN=-1` (the
//! other way round, for testing).

// Without the SDK only the stub is used; the rest is kept and tested.
#![cfg_attr(not(ngx), allow(dead_code))]

/// What the title gives one upscale, as host resources in DLSS's states.
pub(crate) struct Inputs<'a> {
    pub colour: &'a windows::Win32::Graphics::Direct3D12::ID3D12Resource,
    pub depth: &'a windows::Win32::Graphics::Direct3D12::ID3D12Resource,
    pub motion: &'a windows::Win32::Graphics::Direct3D12::ID3D12Resource,
    pub exposure: Option<&'a windows::Win32::Graphics::Direct3D12::ID3D12Resource>,
    pub output: &'a windows::Win32::Graphics::Direct3D12::ID3D12Resource,
    pub jitter: [f32; 2],
    pub motion_scale: [f32; 2],
    pub render: [u32; 2],
    pub output_size: [u32; 2],
    pub reset: bool,
    pub pre_exposure: f32,
    pub frame_ms: f32,
    /// The title's FidelityFX creation flags.
    pub create_flags: u32,
}

/// FidelityFX creation flags (`ffx_upscale.h`, `FFX_UPSCALE_ENABLE_*`).
pub(crate) mod ffx {
    pub const HIGH_DYNAMIC_RANGE: u32 = 1 << 0;
    pub const DISPLAY_RESOLUTION_MOTION_VECTORS: u32 = 1 << 1;
    pub const MOTION_VECTORS_JITTER_CANCELLATION: u32 = 1 << 2;
    pub const DEPTH_INVERTED: u32 = 1 << 3;
    pub const AUTO_EXPOSURE: u32 = 1 << 5;
}

/// NGX's DLSS feature flags (`NVSDK_NGX_DLSS_Feature_Flags_*`) for the
/// title's FidelityFX creation flags.
pub(crate) fn feature_flags(create: u32, exposure_texture: bool) -> i32 {
    const HDR: i32 = 1 << 0;
    const MV_LOW_RES: i32 = 1 << 1;
    const MV_JITTERED: i32 = 1 << 2;
    const DEPTH_INVERTED: i32 = 1 << 3;
    const AUTO_EXPOSURE: i32 = 1 << 6;
    let mut flags = 0;
    if create & ffx::HIGH_DYNAMIC_RANGE != 0 {
        flags |= HDR;
    }
    if create & ffx::DISPLAY_RESOLUTION_MOTION_VECTORS == 0 {
        flags |= MV_LOW_RES;
    }
    // FidelityFX cancels the jitter in motion vectors that carry it.
    if create & ffx::MOTION_VECTORS_JITTER_CANCELLATION != 0 {
        flags |= MV_JITTERED;
    }
    if create & ffx::DEPTH_INVERTED != 0 {
        flags |= DEPTH_INVERTED;
    }
    if create & ffx::AUTO_EXPOSURE != 0 || !exposure_texture {
        flags |= AUTO_EXPOSURE;
    }
    flags
}

/// NGX's quality mode for a render/output width ratio, as FidelityFX's modes
/// make them (quality 1/1.5, balanced 1/1.7, performance 1/2, ultra 1/3).
pub(crate) fn quality(render_width: u32, output_width: u32) -> i32 {
    if output_width == 0 {
        return 2;
    }
    match render_width as f32 / output_width as f32 {
        r if r >= 0.95 => 5, // DLAA
        r if r >= 0.62 => 2, // quality
        r if r >= 0.54 => 1, // balanced
        r if r >= 0.45 => 0, // performance
        _ => 3,              // ultra performance
    }
}

/// `VICTORY_DLSS_PRESET`: `J`=10, `K`=11, `L`=12, `M`=13 or a number.
pub(crate) fn preset(value: &str) -> Option<u32> {
    let v = value.trim().to_ascii_uppercase();
    match v.as_str() {
        "" | "0" | "DEFAULT" => None,
        "J" => Some(10),
        "K" => Some(11),
        "L" => Some(12),
        "M" => Some(13),
        _ => v.parse().ok().filter(|&n| n > 0),
    }
}

#[cfg(ngx)]
mod sdk {
    use std::ffi::{CString, c_char, c_int, c_uint, c_void};
    use std::sync::Mutex;

    use windows::Win32::Graphics::Direct3D12::{ID3D12Device, ID3D12GraphicsCommandList};
    use windows::core::Interface;

    use super::Inputs;

    const SUCCESS: c_int = 1;
    const FEATURE_SUPER_SAMPLING: c_int = 1;
    const ENGINE_CUSTOM: c_int = 0;
    const VERSION_API: c_int = 0x15;

    #[repr(C)]
    struct PathList {
        path: *const *const u16,
        length: c_uint,
    }

    #[repr(C)]
    struct Logging {
        callback: *const c_void,
        minimum: c_int,
        disable_other_sinks: bool,
    }

    #[repr(C)]
    struct CommonInfo {
        paths: PathList,
        internal: *const c_void,
        logging: Logging,
    }

    unsafe extern "C" {
        fn NVSDK_NGX_D3D12_Init_with_ProjectID(
            project: *const c_char,
            engine: c_int,
            engine_version: *const c_char,
            data_path: *const u16,
            device: *mut c_void,
            info: *const CommonInfo,
            version: c_int,
        ) -> c_int;
        fn NVSDK_NGX_D3D12_GetCapabilityParameters(out: *mut *mut c_void) -> c_int;
        fn NVSDK_NGX_D3D12_CreateFeature(list: *mut c_void, feature: c_int, params: *mut c_void, out: *mut *mut c_void) -> c_int;
        fn NVSDK_NGX_D3D12_ReleaseFeature(handle: *mut c_void) -> c_int;
        fn NVSDK_NGX_D3D12_EvaluateFeature_C(list: *mut c_void, handle: *const c_void, params: *const c_void, callback: *const c_void) -> c_int;
        fn NVSDK_NGX_Parameter_SetUI(p: *mut c_void, name: *const c_char, value: c_uint);
        fn NVSDK_NGX_Parameter_SetI(p: *mut c_void, name: *const c_char, value: c_int);
        fn NVSDK_NGX_Parameter_SetF(p: *mut c_void, name: *const c_char, value: f32);
        fn NVSDK_NGX_Parameter_SetD3d12Resource(p: *mut c_void, name: *const c_char, value: *mut c_void);
        fn NVSDK_NGX_Parameter_GetI(p: *mut c_void, name: *const c_char, out: *mut c_int) -> c_int;
    }

    /// A parameter name as the SDK spells it.
    macro_rules! name {
        ($s:literal) => {
            concat!($s, "\0").as_ptr().cast::<c_char>()
        };
    }

    struct Feature {
        handle: *mut c_void,
        /// input width and height, output width and height, flags
        key: [u32; 5],
    }

    struct Dlss {
        params: *mut c_void,
        feature: Option<Feature>,
    }

    // SAFETY: the SDK's objects are used under the mutex only.
    unsafe impl Send for Dlss {}

    /// `None` until tried; `Some(None)`: unavailable, not tried again.
    static DLSS: Mutex<Option<Option<Dlss>>> = Mutex::new(None);

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        s.encode_wide().chain([0]).collect()
    }

    fn env_f(name: &str, default: f32) -> f32 {
        std::env::var(name).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(default)
    }

    /// The folder `nvngx_dlss.dll` is loaded from.
    fn dll_dir() -> Option<std::path::PathBuf> {
        std::env::var_os("VICTORY_DLSS_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::current_exe().ok()?.parent().map(std::path::Path::to_path_buf))
    }

    fn init(device: &ID3D12Device) -> Option<Dlss> {
        const DLL: &str = "nvngx_dlss.dll";
        let dir = dll_dir()?;
        // Without a copy of its own, NGX's driver copy took an access violation
        // in `NVSDK_NGX_D3D12_Init_with_ProjectID` on 610+ drivers
        // (aleksesipenko/VIctory-RTX, after TaynanGT #186): not tried.
        if !dir.join(DLL).is_file() {
            tracing::warn!(dir = %dir.display(), "DLSS: no {DLL} in VICTORY_DLSS_DIR (or beside the executable); the title's upscale is not run");
            return None;
        }
        let data = std::env::temp_dir().join("victory-ngx");
        let _ = std::fs::create_dir_all(&data);
        let (dir_w, data_w) = (wide(dir.as_os_str()), wide(data.as_os_str()));
        let paths = [dir_w.as_ptr()];
        let info = CommonInfo {
            paths: PathList { path: paths.as_ptr(), length: 1 },
            internal: std::ptr::null(),
            logging: Logging { callback: std::ptr::null(), minimum: 0, disable_other_sinks: false },
        };
        // Any id: `ENGINE_CUSTOM` takes a project's own.
        let project = CString::new("8f3d5c2a-6b1e-4f7a-9c0d-2e4b6a8c1d3f").ok()?;
        let version = CString::new("1.0").ok()?;
        // SAFETY: live strings and device; the SDK copies what it keeps.
        let r = unsafe {
            NVSDK_NGX_D3D12_Init_with_ProjectID(
                project.as_ptr(),
                ENGINE_CUSTOM,
                version.as_ptr(),
                data_w.as_ptr(),
                device.as_raw(),
                &info,
                VERSION_API,
            )
        };
        if r != SUCCESS {
            tracing::warn!(result = format_args!("{r:#x}"), dir = %dir.display(), "DLSS: NGX did not start");
            return None;
        }
        let mut params = std::ptr::null_mut();
        // SAFETY: an out pointer.
        let r = unsafe { NVSDK_NGX_D3D12_GetCapabilityParameters(&mut params) };
        if r != SUCCESS || params.is_null() {
            tracing::warn!(result = format_args!("{r:#x}"), "DLSS: no capability parameters");
            return None;
        }
        let mut available = 0;
        // SAFETY: live parameters.
        unsafe { NVSDK_NGX_Parameter_GetI(params, name!("SuperSampling.Available"), &mut available) };
        if available == 0 {
            tracing::warn!("DLSS: super resolution is not available on this card or driver");
            return None;
        }
        tracing::info!(dir = %dir.display(), "DLSS: started; the title's upscale is DLSS");
        Some(Dlss { params, feature: None })
    }

    /// Whether DLSS can run (started once, on first use).
    pub fn available(device: &ID3D12Device) -> bool {
        let mut guard = DLSS.lock().unwrap_or_else(|e| e.into_inner());
        guard.get_or_insert_with(|| init(device)).is_some()
    }

    /// Releases the feature once the card has run every list that used it.
    pub fn release() {
        let mut guard = DLSS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(Some(dlss)) = guard.as_mut()
            && let Some(feature) = dlss.feature.take()
        {
            victory_gpu::external::finish_submitted();
            // SAFETY: made by CreateFeature; no list in flight uses it.
            unsafe { NVSDK_NGX_D3D12_ReleaseFeature(feature.handle) };
        }
    }

    /// Records DLSS over `inputs` into `list`; false if it could not.
    pub fn record(device: &ID3D12Device, list: &ID3D12GraphicsCommandList, inputs: &Inputs<'_>) -> bool {
        let mut guard = DLSS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(dlss) = guard.get_or_insert_with(|| init(device)) else {
            return false;
        };
        // SAFETY: a live resource.
        let colour = unsafe { inputs.colour.GetDesc() };
        // The feature takes the largest size the colour texture holds, so that
        // a render size that changes inside it needs no new feature.
        let (in_w, in_h) = (colour.Width as u32, colour.Height);
        let flags = super::feature_flags(inputs.create_flags, inputs.exposure.is_some());
        let [out_w, out_h] = inputs.output_size;
        let key = [in_w, in_h, out_w, out_h, flags as u32];
        let p = dlss.params;
        if dlss.feature.as_ref().is_none_or(|f| f.key != key) {
            if let Some(old) = dlss.feature.take() {
                victory_gpu::external::finish_submitted();
                // SAFETY: made by CreateFeature; no list in flight uses it.
                unsafe { NVSDK_NGX_D3D12_ReleaseFeature(old.handle) };
            }
            let quality = std::env::var("VICTORY_DLSS_QUALITY")
                .ok()
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or_else(|| super::quality(inputs.render[0], out_w));
            let preset = std::env::var("VICTORY_DLSS_PRESET").ok().and_then(|v| super::preset(&v));
            let mut handle = std::ptr::null_mut();
            // SAFETY: live parameters and an open list.
            let r = unsafe {
                NVSDK_NGX_Parameter_SetUI(p, name!("CreationNodeMask"), 1);
                NVSDK_NGX_Parameter_SetUI(p, name!("VisibilityNodeMask"), 1);
                NVSDK_NGX_Parameter_SetUI(p, name!("Width"), in_w);
                NVSDK_NGX_Parameter_SetUI(p, name!("Height"), in_h);
                NVSDK_NGX_Parameter_SetUI(p, name!("OutWidth"), out_w);
                NVSDK_NGX_Parameter_SetUI(p, name!("OutHeight"), out_h);
                NVSDK_NGX_Parameter_SetI(p, name!("PerfQualityValue"), quality);
                NVSDK_NGX_Parameter_SetI(p, name!("DLSS.Feature.Create.Flags"), flags);
                NVSDK_NGX_Parameter_SetI(p, name!("DLSS.Enable.Output.Subrects"), 0);
                if let Some(code) = preset {
                    for hint in [
                        name!("DLSS.Hint.Render.Preset.DLAA"),
                        name!("DLSS.Hint.Render.Preset.Quality"),
                        name!("DLSS.Hint.Render.Preset.Balanced"),
                        name!("DLSS.Hint.Render.Preset.Performance"),
                        name!("DLSS.Hint.Render.Preset.UltraPerformance"),
                        name!("DLSS.Hint.Render.Preset.UltraQuality"),
                    ] {
                        NVSDK_NGX_Parameter_SetUI(p, hint, code);
                    }
                }
                NVSDK_NGX_D3D12_CreateFeature(list.as_raw(), FEATURE_SUPER_SAMPLING, p, &mut handle)
            };
            if r != SUCCESS || handle.is_null() {
                tracing::warn!(result = format_args!("{r:#x}"), ?key, "DLSS: no feature for these sizes");
                return false;
            }
            tracing::info!(
                input = ?(in_w, in_h),
                render = ?inputs.render,
                output = ?inputs.output_size,
                flags = format_args!("{flags:#x}"),
                title_flags = format_args!("{:#x}", inputs.create_flags),
                quality,
                ?preset,
                colour_format = colour.Format.0,
                "DLSS: feature made"
            );
            dlss.feature = Some(Feature { handle, key });
        }
        let Some(feature) = dlss.feature.as_ref() else { return false };
        let mv = env_f("VICTORY_DLSS_MV_SIGN", 1.0);
        let jitter = env_f("VICTORY_DLSS_JITTER_SIGN", 1.0);
        let scale = |s: f32| if s == 0.0 { 1.0 } else { s };
        let exposure = inputs.exposure.map_or(std::ptr::null_mut(), |r| r.as_raw());
        // SAFETY: live parameters, resources and feature; an open list.
        let r = unsafe {
            NVSDK_NGX_Parameter_SetD3d12Resource(p, name!("Color"), inputs.colour.as_raw());
            NVSDK_NGX_Parameter_SetD3d12Resource(p, name!("Output"), inputs.output.as_raw());
            NVSDK_NGX_Parameter_SetD3d12Resource(p, name!("Depth"), inputs.depth.as_raw());
            NVSDK_NGX_Parameter_SetD3d12Resource(p, name!("MotionVectors"), inputs.motion.as_raw());
            NVSDK_NGX_Parameter_SetD3d12Resource(p, name!("ExposureTexture"), exposure);
            NVSDK_NGX_Parameter_SetF(p, name!("Jitter.Offset.X"), jitter * inputs.jitter[0]);
            NVSDK_NGX_Parameter_SetF(p, name!("Jitter.Offset.Y"), jitter * inputs.jitter[1]);
            NVSDK_NGX_Parameter_SetF(p, name!("MV.Scale.X"), mv * scale(inputs.motion_scale[0]));
            NVSDK_NGX_Parameter_SetF(p, name!("MV.Scale.Y"), mv * scale(inputs.motion_scale[1]));
            NVSDK_NGX_Parameter_SetI(p, name!("Reset"), c_int::from(inputs.reset));
            NVSDK_NGX_Parameter_SetF(p, name!("DLSS.Pre.Exposure"), if inputs.pre_exposure > 0.0 { inputs.pre_exposure } else { 1.0 });
            NVSDK_NGX_Parameter_SetF(p, name!("FrameTimeDeltaInMsec"), inputs.frame_ms);
            NVSDK_NGX_Parameter_SetUI(p, name!("DLSS.Render.Subrect.Dimensions.Width"), inputs.render[0].min(in_w));
            NVSDK_NGX_Parameter_SetUI(p, name!("DLSS.Render.Subrect.Dimensions.Height"), inputs.render[1].min(in_h));
            NVSDK_NGX_D3D12_EvaluateFeature_C(list.as_raw(), feature.handle, p, std::ptr::null())
        };
        if r != SUCCESS {
            static TOLD: std::sync::Once = std::sync::Once::new();
            TOLD.call_once(|| tracing::warn!(result = format_args!("{r:#x}"), "DLSS: an evaluation failed"));
            return false;
        }
        true
    }
}

#[cfg(ngx)]
pub(crate) use sdk::{available, record, release};

/// Whether this build has DLSS (the `dlss` feature and the SDK at build time).
pub const BUILT: bool = cfg!(ngx);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_titles_flags_become_the_features() {
        // Low-res motion vectors, no jitter in them, an exposure texture: none of the others.
        assert_eq!(feature_flags(0, true), 1 << 1);
        // Without an exposure texture DLSS finds the exposure itself.
        assert_eq!(feature_flags(0, false), (1 << 1) | (1 << 6));
        let all = ffx::HIGH_DYNAMIC_RANGE
            | ffx::DISPLAY_RESOLUTION_MOTION_VECTORS
            | ffx::MOTION_VECTORS_JITTER_CANCELLATION
            | ffx::DEPTH_INVERTED
            | ffx::AUTO_EXPOSURE;
        assert_eq!(feature_flags(all, true), 1 | (1 << 2) | (1 << 3) | (1 << 6));
    }

    #[test]
    fn quality_follows_fidelityfxs_ratios() {
        assert_eq!(quality(1920, 1920), 5);
        assert_eq!(quality(1280, 1920), 2);
        assert_eq!(quality(1129, 1920), 1);
        assert_eq!(quality(960, 1920), 0);
        assert_eq!(quality(640, 1920), 3);
        assert_eq!(quality(640, 0), 2);
    }

    #[test]
    fn presets_by_letter_or_number() {
        assert_eq!(preset("k"), Some(11));
        assert_eq!(preset(" M "), Some(13));
        assert_eq!(preset("14"), Some(14));
        assert_eq!(preset("default"), None);
        assert_eq!(preset(""), None);
        assert_eq!(preset("x"), None);
    }
}

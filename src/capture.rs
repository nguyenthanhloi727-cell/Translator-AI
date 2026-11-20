//! Screen/window capture through the standard Windows Graphics Capture API.
//! No hooks, no injection: the same mechanism OBS uses for "Window Capture".

use crate::frame::{Frame, Region, fnv1a};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, POINT};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, IsWindowVisible,
};
use windows::core::{BOOL, Interface};

pub enum Target {
    Window(HWND),
    PrimaryMonitor,
}

/// Per-frame measurements sent to the caller.
pub struct FrameStat {
    /// Time from the frame being composed by Windows until our handler started.
    pub arrive_ms: f64,
    /// Time to copy the (cropped) area from GPU to CPU memory.
    pub copy_ms: f64,
    /// False when the cropped area is identical to the previous frame.
    pub changed: bool,
}

pub struct Capture {
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
}

impl Capture {
    pub fn stop(self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

struct Gpu {
    device: ID3D11Device,
    ctx: ID3D11DeviceContext,
}

fn create_gpu() -> windows::core::Result<Gpu> {
    let mut last_err = None;
    // Hardware first; fall back to the CPU rasterizer so the app still runs on weak PCs.
    for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
        let mut device = None;
        let mut ctx = None;
        let r = unsafe {
            D3D11CreateDevice(
                None,
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut ctx),
            )
        };
        match r {
            Ok(()) => {
                return Ok(Gpu { device: device.unwrap(), ctx: ctx.unwrap() });
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap())
}

fn qpc_now_100ns() -> i64 {
    let (mut c, mut f) = (0i64, 0i64);
    unsafe {
        let _ = QueryPerformanceCounter(&mut c);
        let _ = QueryPerformanceFrequency(&mut f);
    }
    (c as i128 * 10_000_000 / f as i128) as i64
}

/// Visible top-level windows that have a title.
pub fn list_windows() -> Vec<(HWND, String)> {
    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let out = unsafe { &mut *(lparam.0 as *mut Vec<(HWND, String)>) };
        if unsafe { IsWindowVisible(hwnd) }.as_bool() {
            let mut buf = [0u16; 256];
            let n = unsafe { GetWindowTextW(hwnd, &mut buf) } as usize;
            if n > 0 {
                out.push((hwnd, String::from_utf16_lossy(&buf[..n])));
            }
        }
        true.into()
    }
    let mut out: Vec<(HWND, String)> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut out as *mut _ as isize));
    }
    out
}

pub fn find_window(needle: &str) -> Option<HWND> {
    let needle = needle.to_lowercase();
    list_windows()
        .into_iter()
        .find(|(_, t)| t.to_lowercase().contains(&needle))
        .map(|(h, _)| h)
}

/// Start capturing. For every frame a `FrameStat` is sent on `tx`, and the latest
/// cropped frame is kept in `last`.
pub fn start(
    target: Target,
    region: Option<Region>,
    tx: Sender<FrameStat>,
    last: Arc<Mutex<Option<Frame>>>,
) -> windows::core::Result<Capture> {
    let gpu = create_gpu()?;
    let dxgi: IDXGIDevice = gpu.device.cast()?;
    let d3d: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?;

    let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    let item: GraphicsCaptureItem = match target {
        Target::Window(hwnd) => unsafe { interop.CreateForWindow(hwnd)? },
        Target::PrimaryMonitor => unsafe {
            let mon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            interop.CreateForMonitor(mon)?
        },
    };

    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &d3d,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        2,
        item.Size()?,
    )?;
    let session = pool.CreateCaptureSession(&item)?;
    let _ = session.SetIsCursorCaptureEnabled(false);

    struct State {
        ctx: ID3D11DeviceContext,
        staging: Option<(ID3D11Texture2D, u32, u32)>,
        prev_hash: u64,
    }
    let state = Mutex::new(State { ctx: gpu.ctx.clone(), staging: None, prev_hash: 0 });
    let device = gpu.device.clone();

    pool.FrameArrived(&TypedEventHandler::new(
        move |pool: windows::core::Ref<Direct3D11CaptureFramePool>, _| {
            let started = Instant::now();
            let Some(pool) = pool.as_ref() else { return Ok(()) };
            let frame = pool.TryGetNextFrame()?;
            let arrive_ms =
                (qpc_now_100ns() - frame.SystemRelativeTime()?.Duration) as f64 / 10_000.0;

            let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
            let tex: ID3D11Texture2D = unsafe { access.GetInterface()? };
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            unsafe { tex.GetDesc(&mut desc) };

            let full = Region { x: 0, y: 0, w: desc.Width, h: desc.Height };
            let Some(r) = region.map_or(Some(full), |r| r.clamp(desc.Width, desc.Height)) else {
                return Ok(());
            };

            let mut st = state.lock().unwrap();
            if st.staging.as_ref().map(|s| (s.1, s.2)) != Some((r.w, r.h)) {
                let sd = D3D11_TEXTURE2D_DESC {
                    Width: r.w,
                    Height: r.h,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: D3D11_USAGE_STAGING,
                    BindFlags: 0,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    MiscFlags: 0,
                };
                let mut t = None;
                unsafe { device.CreateTexture2D(&sd, None, Some(&mut t))? };
                st.staging = Some((t.unwrap(), r.w, r.h));
            }
            let staging = st.staging.as_ref().unwrap().0.clone();
            let bx = D3D11_BOX {
                left: r.x,
                top: r.y,
                front: 0,
                right: r.x + r.w,
                bottom: r.y + r.h,
                back: 1,
            };
            let mut bgra = vec![0u8; (r.w * r.h * 4) as usize];
            unsafe {
                st.ctx.CopySubresourceRegion(&staging, 0, 0, 0, 0, &tex, 0, Some(&bx));
                let mut m = D3D11_MAPPED_SUBRESOURCE::default();
                st.ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut m))?;
                let row = (r.w * 4) as usize;
                for y in 0..r.h as usize {
                    let src = (m.pData as *const u8).add(y * m.RowPitch as usize);
                    std::ptr::copy_nonoverlapping(src, bgra.as_mut_ptr().add(y * row), row);
                }
                st.ctx.Unmap(&staging, 0);
            }
            let copy_ms = started.elapsed().as_secs_f64() * 1000.0;

            let h = fnv1a(&bgra);
            let changed = h != st.prev_hash;
            st.prev_hash = h;
            drop(st);

            *last.lock().unwrap() = Some(Frame { width: r.w, height: r.h, bgra });
            let _ = tx.send(FrameStat { arrive_ms, copy_ms, changed });
            Ok(())
        },
    ))?;
    session.StartCapture()?;
    Ok(Capture { pool, session })
}

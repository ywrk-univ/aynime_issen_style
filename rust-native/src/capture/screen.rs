use anyhow::{Context, Result};
use windows::core::Interface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE,
    D3D11_MAP_READ, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    IDXGIAdapter1, IDXGIDevice, IDXGIOutput, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
    DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
};

/// Raw screen frame in BGRA format.
pub struct ScreenFrame {
    pub width: u32,
    pub height: u32,
    /// Pixel data in BGRA8 format, row-major.
    pub data: Vec<u8>,
}

/// DXGI Desktop Duplication based screen capturer.
/// Automatically re-creates the duplication session if it becomes invalid
/// (e.g. after desktop composition changes like fullscreen overlay windows).
pub struct ScreenCapturer {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    output_width: u32,
    output_height: u32,
}

impl ScreenCapturer {
    /// Create a new screen capturer for the primary monitor.
    pub fn new() -> Result<Self> {
        let mut device = None;
        let mut context = None;

        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                7, // D3D11_SDK_VERSION
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .context("Failed to create D3D11 device")?;

        let device = device.context("D3D11 device is None")?;
        let context = context.context("D3D11 device context is None")?;

        let dxgi_device: IDXGIDevice = device.cast().context("Failed to get IDXGIDevice")?;
        let adapter: IDXGIAdapter1 = unsafe { dxgi_device.GetAdapter() }
            .context("Failed to get adapter")?
            .cast()
            .context("Failed to cast to IDXGIAdapter1")?;

        let output: IDXGIOutput = unsafe { adapter.EnumOutputs(0) }
            .context("Failed to enumerate outputs - no monitor found")?;

        let output_desc =
            unsafe { output.GetDesc() }.context("Failed to get output description")?;
        let output_width =
            (output_desc.DesktopCoordinates.right - output_desc.DesktopCoordinates.left) as u32;
        let output_height =
            (output_desc.DesktopCoordinates.bottom - output_desc.DesktopCoordinates.top) as u32;

        log::info!(
            "Screen capture target: {}x{}",
            output_width,
            output_height
        );

        let output1: IDXGIOutput1 = output
            .cast()
            .context("Failed to cast to IDXGIOutput1 - DXGI 1.2+ required")?;

        let duplication = unsafe { output1.DuplicateOutput(&device) }
            .context("Failed to create output duplication. Is another app using it?")?;

        Ok(Self {
            device,
            context,
            duplication,
            output_width,
            output_height,
        })
    }

    /// Re-create the duplication session using the existing D3D11 device.
    /// Called automatically when ACCESS_LOST is detected.
    fn recreate_duplication(&mut self) -> Result<()> {
        log::info!("Re-creating DXGI output duplication session...");

        // Release old duplication
        let _ = unsafe { self.duplication.ReleaseFrame() };

        let dxgi_device: IDXGIDevice = self.device.cast().context("Failed to get IDXGIDevice")?;
        let adapter: IDXGIAdapter1 = unsafe { dxgi_device.GetAdapter() }
            .context("Failed to get adapter")?
            .cast()
            .context("Failed to cast to IDXGIAdapter1")?;

        let output: IDXGIOutput = unsafe { adapter.EnumOutputs(0) }
            .context("Failed to enumerate outputs")?;

        let output1: IDXGIOutput1 = output
            .cast()
            .context("Failed to cast to IDXGIOutput1")?;

        self.duplication = unsafe { output1.DuplicateOutput(&self.device) }
            .context("Failed to re-create output duplication")?;

        log::info!("DXGI output duplication session re-created");
        Ok(())
    }

    /// Capture the current screen frame.
    /// Returns `None` if no new frame is available (timeout).
    /// Automatically re-creates the duplication session on ACCESS_LOST.
    pub fn capture_frame(&mut self) -> Result<Option<ScreenFrame>> {
        match self.try_capture_frame() {
            Ok(result) => Ok(result),
            Err(e) => {
                // Check if this is an ACCESS_LOST error
                let err_str = format!("{:#}", e);
                if err_str.contains("ACCESS_LOST") || err_str.contains("0x887A0026") {
                    log::warn!("DXGI ACCESS_LOST detected, re-creating session...");
                    self.recreate_duplication()?;
                    // After re-creation, first frame may not be ready yet
                    Ok(None)
                } else {
                    Err(e)
                }
            }
        }
    }

    fn try_capture_frame(&mut self) -> Result<Option<ScreenFrame>> {
        let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut desktop_resource: Option<IDXGIResource> = None;

        let result = unsafe {
            self.duplication
                .AcquireNextFrame(100, &mut frame_info, &mut desktop_resource)
        };

        match result {
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(None),
            Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => {
                anyhow::bail!("DXGI_ERROR_ACCESS_LOST");
            }
            Err(e) => return Err(e).context("Failed to acquire next frame"),
            Ok(()) => {}
        }

        let desktop_resource = desktop_resource.context("Desktop resource is None")?;
        let desktop_texture: ID3D11Texture2D = desktop_resource
            .cast()
            .context("Failed to cast resource to texture")?;

        let staging_desc = D3D11_TEXTURE2D_DESC {
            Width: self.output_width,
            Height: self.output_height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0u32,
        };

        let staging_texture = {
            let mut tex = None;
            unsafe {
                self.device
                    .CreateTexture2D(&staging_desc, None, Some(&mut tex))
            }
            .context("Failed to create staging texture")?;
            tex.context("Staging texture is None")?
        };

        unsafe {
            self.context
                .CopyResource(&staging_texture, &desktop_texture);
        }

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context
                .Map(&staging_texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        }
        .context("Failed to map staging texture")?;

        let row_pitch = mapped.RowPitch as usize;
        let width = self.output_width as usize;
        let height = self.output_height as usize;

        let mut data = Vec::with_capacity(width * height * 4);
        let src = mapped.pData as *const u8;
        for y in 0..height {
            let row = unsafe {
                let row_start = src.add(y * row_pitch);
                std::slice::from_raw_parts(row_start, width * 4)
            };
            data.extend_from_slice(row);
        }

        unsafe {
            self.context.Unmap(&staging_texture, 0);
        }
        unsafe {
            self.duplication.ReleaseFrame()
        }
        .context("Failed to release frame")?;

        Ok(Some(ScreenFrame {
            width: self.output_width,
            height: self.output_height,
            data,
        }))
    }

    /// Get the screen dimensions.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.output_width, self.output_height)
    }
}

impl Drop for ScreenCapturer {
    fn drop(&mut self) {
        let _ = unsafe { self.duplication.ReleaseFrame() };
    }
}

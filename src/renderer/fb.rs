//! Framebuffer abstraction.
//!
//! On Miyoo Mini Plus (arm, no `desktop` feature): wraps /dev/fb0 as a
//! memory-mapped BGRA buffer. The screen is rotated 180° in hardware.
//!
//! On desktop (`desktop` feature, default): uses a `minifb` window. The pixel
//! buffer is stored in a thread-local so `InputState::poll()` can pump window
//! events without holding a `&mut Framebuffer`.

/// BGRA colour — matches the Miyoo framebuffer byte order exactly.
/// Also used by all drawing code on desktop (converted to XRGB on write).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bgra {
    pub b: u8,
    pub g: u8,
    pub r: u8,
}

impl Bgra {
    pub const fn new(r: u8, g: u8, b: u8) -> Self { Self { b, g, r } }
    pub const fn black()  -> Self { Self::new(0, 0, 0) }
    pub const fn white()  -> Self { Self::new(255, 255, 255) }
    pub const fn red()    -> Self { Self::new(200, 50, 50) }
    pub const fn green()  -> Self { Self::new(50, 200, 50) }
    pub const fn blue()   -> Self { Self::new(50, 100, 200) }
    pub const fn yellow() -> Self { Self::new(255, 220, 0) }
    pub const fn sky()    -> Self { Self::new(100, 150, 210) }
    pub const fn earth()  -> Self { Self::new(80, 55, 30) }
    pub const fn grass()  -> Self { Self::new(60, 130, 40) }
    pub const fn water()  -> Self { Self::new(30, 80, 180) }
}

// ── Desktop (minifb) implementation ──────────────────────────────────────────

#[cfg(feature = "desktop")]
mod desktop_impl {
    use std::cell::RefCell;
    use minifb::{Window, WindowOptions, Scale};

    pub(super) struct State {
        pub window: Window,
        pub pixels: Vec<u32>,
        pub width:  u32,
        pub height: u32,
    }

    thread_local! {
        pub(super) static WIN: RefCell<Option<State>> = RefCell::new(None);
    }

    pub fn init(width: u32, height: u32) {
        let mut opts = WindowOptions::default();
        opts.scale = Scale::X2;
        let window = Window::new("Mini Mayhem", width as usize, height as usize, opts)
            .expect("failed to create minifb window");
        let pixels = vec![0u32; (width * height) as usize];
        WIN.with(|w| *w.borrow_mut() = Some(State { window, pixels, width, height }));
    }

    /// Copy one horizontal scanline (BGRA bytes) into the u32 pixel buffer.
    pub fn blit_row(screen_y: u32, src: &[u8], width: u32) {
        WIN.with(|w| {
            if let Some(ref mut s) = *w.borrow_mut() {
                let row = (screen_y * width) as usize;
                for (x, pixel) in src.chunks_exact(4).enumerate() {
                    let b = pixel[0] as u32;
                    let g = pixel[1] as u32;
                    let r = pixel[2] as u32;
                    s.pixels[row + x] = (r << 16) | (g << 8) | b;
                }
            }
        });
    }

    /// Push the pixel buffer to the window and process OS events.
    /// Also called from InputState::poll() to keep the message queue drained
    /// during loops that poll input without redrawing (splash, confirm prompts).
    pub fn present() {
        WIN.with(|w| {
            if let Some(ref mut s) = *w.borrow_mut() {
                let _ = s.window.update_with_buffer(&s.pixels, s.width as usize, s.height as usize);
            }
        });
    }

    pub fn is_open() -> bool {
        WIN.with(|w| w.borrow().as_ref().map(|s| s.window.is_open()).unwrap_or(false))
    }

    pub fn get_keys_pressed() -> Vec<minifb::Key> {
        WIN.with(|w| {
            w.borrow().as_ref().map(|s| {
                s.window.get_keys_pressed(minifb::KeyRepeat::No)
            }).unwrap_or_default()
        })
    }

    pub fn get_keys_released() -> Vec<minifb::Key> {
        WIN.with(|w| {
            w.borrow().as_ref().map(|s| s.window.get_keys_released()).unwrap_or_default()
        })
    }

    pub fn get_keys() -> Vec<minifb::Key> {
        WIN.with(|w| {
            w.borrow().as_ref().map(|s| s.window.get_keys()).unwrap_or_default()
        })
    }
}

/// Pump the window event queue without presenting a new frame (desktop only).
/// Call from `InputState::poll()` so input-polling loops don't starve the OS.
pub fn pump_events() {
    #[cfg(feature = "desktop")]
    desktop_impl::present();
}

/// Keys just pressed this frame (desktop only — returns empty on Miyoo).
#[cfg(feature = "desktop")]
pub fn desktop_keys_pressed() -> Vec<minifb::Key> {
    desktop_impl::get_keys_pressed()
}

/// Keys just released this frame (desktop only).
#[cfg(feature = "desktop")]
pub fn desktop_keys_released() -> Vec<minifb::Key> {
    desktop_impl::get_keys_released()
}

/// Keys currently held (desktop only).
#[cfg(feature = "desktop")]
pub fn desktop_keys_held() -> Vec<minifb::Key> {
    desktop_impl::get_keys()
}

/// True while the desktop window is open (always true on Miyoo).
pub fn is_window_open() -> bool {
    #[cfg(feature = "desktop")]
    return desktop_impl::is_open();
    #[cfg(not(feature = "desktop"))]
    true
}

// ── Desktop Framebuffer ───────────────────────────────────────────────────────

#[cfg(feature = "desktop")]
pub struct Framebuffer {
    pub width:  u32,
    pub height: u32,
}

#[cfg(feature = "desktop")]
impl Framebuffer {
    pub fn open() -> Result<Self, String> {
        let (w, h) = (640u32, 480u32);
        desktop_impl::init(w, h);
        Ok(Self { width: w, height: h })
    }

    pub fn blit_row(&mut self, screen_y: u32, src: &[u8]) {
        desktop_impl::blit_row(screen_y, src, self.width);
    }

    pub fn screen_w(&self) -> u32 { self.width }
    pub fn screen_h(&self) -> u32 { self.height }

    /// Push the pixel buffer to the window. Called at the end of every blit.
    pub fn present(&mut self) { desktop_impl::present(); }

    /// True while the window hasn't been closed.
    pub fn is_open(&self) -> bool { desktop_impl::is_open() }

    // Miyoo-only helpers used by updater — stubs on desktop.
    pub fn set_pixel(&mut self, _x: u32, _y: u32, _colour: Bgra) {}
    pub fn clear(&mut self, _colour: Bgra) {}
}

// ── Miyoo (non-desktop) implementation ───────────────────────────────────────

#[cfg(not(feature = "desktop"))]
const FBIOGET_VSCREENINFO: libc::c_ulong = 0x4600;

#[cfg(not(feature = "desktop"))]
#[repr(C)]
struct FbVarScreenInfo {
    xres:           u32,
    yres:           u32,
    xres_virtual:   u32,
    yres_virtual:   u32,
    xoffset:        u32,
    yoffset:        u32,
    bits_per_pixel: u32,
    _pad: [u8; 132],
}

#[cfg(not(feature = "desktop"))]
pub struct Framebuffer {
    fd:       libc::c_int,
    buf:      &'static mut [u8],
    flip_buf: Vec<u8>,
    pub width:  u32,
    pub height: u32,
    stride: u32,
}

#[cfg(not(feature = "desktop"))]
impl Framebuffer {
    pub fn open() -> Result<Self, String> {
        use libc::{open, mmap, ioctl, O_RDWR, PROT_READ, PROT_WRITE, MAP_SHARED};

        let path = b"/dev/fb0\0";
        let fd = unsafe { open(path.as_ptr() as *const libc::c_char, O_RDWR) };
        if fd < 0 {
            return Err(format!("open /dev/fb0 failed: errno {}", unsafe { *libc::__errno_location() }));
        }

        let mut info = FbVarScreenInfo {
            xres: 0, yres: 0, xres_virtual: 0, yres_virtual: 0,
            xoffset: 0, yoffset: 0, bits_per_pixel: 0, _pad: [0u8; 132],
        };

        if unsafe { ioctl(fd, FBIOGET_VSCREENINFO, &mut info) } < 0 {
            unsafe { libc::close(fd) };
            return Err("FBIOGET_VSCREENINFO ioctl failed".into());
        }

        let width  = info.xres;
        let height = info.yres;
        let bpp    = info.bits_per_pixel / 8;
        let stride = width * bpp;
        let size   = (stride * height) as usize;

        let ptr = unsafe {
            mmap(std::ptr::null_mut(), size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0)
        };

        if ptr == libc::MAP_FAILED {
            unsafe { libc::close(fd) };
            return Err("mmap /dev/fb0 failed".into());
        }

        let buf = unsafe { std::slice::from_raw_parts_mut(ptr as *mut u8, size) };
        Ok(Self { fd, buf, flip_buf: vec![0u8; size], width, height, stride })
    }

    #[inline(always)]
    pub fn set_pixel(&mut self, x: u32, y: u32, colour: Bgra) {
        if x >= self.width || y >= self.height { return; }
        let off = (y * self.stride + x * 4) as usize;
        self.buf[off]     = colour.b;
        self.buf[off + 1] = colour.g;
        self.buf[off + 2] = colour.r;
        self.buf[off + 3] = 0xFF;
    }

    pub fn clear(&mut self, colour: Bgra) {
        for i in (0..self.buf.len()).step_by(4) {
            self.buf[i]     = colour.b;
            self.buf[i + 1] = colour.g;
            self.buf[i + 2] = colour.r;
            self.buf[i + 3] = 0xFF;
        }
    }

    pub fn blit_row(&mut self, screen_y: u32, src: &[u8]) {
        if screen_y >= self.height { return; }
        // Miyoo Mini Plus framebuffer is rotated 180°: both axes flipped.
        // Write the reversed row into flip_buf (heap, cache-warm); present()
        // flushes the whole frame to mmap in one copy instead of 480 separate
        // mmap writes.
        let ry = self.height - 1 - screen_y;
        let dst_off = (ry * self.stride) as usize;
        let w = self.width as usize;
        let row_bytes = w * 4;
        let dst_px = unsafe {
            std::slice::from_raw_parts_mut(
                self.flip_buf[dst_off..dst_off + row_bytes].as_mut_ptr() as *mut u32,
                w,
            )
        };
        let src_px = unsafe {
            std::slice::from_raw_parts(src.as_ptr() as *const u32, w)
        };
        for i in 0..w {
            dst_px[w - 1 - i] = src_px[i];
        }
    }

    pub fn screen_w(&self) -> u32 { self.width }
    pub fn screen_h(&self) -> u32 { self.height }

    /// Flush the completed frame from flip_buf to mmap in one contiguous copy.
    pub fn present(&mut self) {
        self.buf.copy_from_slice(&self.flip_buf);
    }

    pub fn is_open(&self) -> bool { true }
}

#[cfg(not(feature = "desktop"))]
impl Drop for Framebuffer {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.buf.as_mut_ptr() as *mut _, self.buf.len());
            libc::close(self.fd);
        }
    }
}

// ── Bgra tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_new_stores_channels_correctly() {
        let c = Bgra::new(255, 128, 64);
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 128);
        assert_eq!(c.b, 64);
    }

    #[test]
    fn bgra_black_is_all_zero() {
        let c = Bgra::black();
        assert_eq!(c.r, 0);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
    }

    #[test]
    fn bgra_white_is_all_max() {
        let c = Bgra::white();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 255);
        assert_eq!(c.b, 255);
    }

    #[test]
    fn bgra_colours_are_distinct() {
        assert_ne!(Bgra::red(),   Bgra::green());
        assert_ne!(Bgra::green(), Bgra::blue());
        assert_ne!(Bgra::blue(),  Bgra::red());
    }

    #[test]
    fn bgra_sky_grass_earth_water_are_distinct() {
        let colours = [Bgra::sky(), Bgra::grass(), Bgra::earth(), Bgra::water()];
        for i in 0..colours.len() {
            for j in 0..colours.len() {
                if i != j {
                    assert_ne!(colours[i], colours[j],
                        "colours[{i}] and colours[{j}] should be distinct");
                }
            }
        }
    }

    #[test]
    fn bgra_yellow_has_no_blue() {
        assert_eq!(Bgra::yellow().b, 0);
    }
}

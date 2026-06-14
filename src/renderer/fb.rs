//! Direct framebuffer access for Miyoo Mini Plus.
//!
//! The device exposes /dev/fb0 as a memory-mapped BGRA buffer.
//! BGRA byte order is the single most common mistake — not RGB.
//!
//!   buf[offset + 0] = blue
//!   buf[offset + 1] = green
//!   buf[offset + 2] = red
//!   buf[offset + 3] = 0xFF  (alpha, ignored by hardware but must be set)
//!
//! Screen size is queried at runtime via FBIOGET_VSCREENINFO ioctl
//! rather than hardcoding 640×480.



/// BGRA colour — matches the Miyoo framebuffer byte order exactly.
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

// ── ioctl constant from <linux/fb.h> ─────────────────────────────────────────

const FBIOGET_VSCREENINFO: libc::c_ulong = 0x4600;

/// Subset of fb_var_screeninfo we care about.
/// Full struct is 160 bytes; we only read the first few fields.
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

/// Direct framebuffer renderer.
/// Wraps /dev/fb0 as a mmap'd BGRA pixel buffer.
pub struct Framebuffer {
    fd:     libc::c_int,
    buf:    &'static mut [u8],
    pub width:  u32,
    pub height: u32,
    stride: u32,  // bytes per row
}

impl Framebuffer {
    /// Open and mmap /dev/fb0. Returns Err on any failure.
    /// Only call this on the actual Miyoo hardware.
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
        let bpp    = info.bits_per_pixel / 8;  // bytes per pixel
        let stride = width * bpp;
        let size   = (stride * height) as usize;

        let ptr = unsafe {
            mmap(
                std::ptr::null_mut(),
                size,
                PROT_READ | PROT_WRITE,
                MAP_SHARED,
                fd,
                0,
            )
        };

        if ptr == libc::MAP_FAILED {
            unsafe { libc::close(fd) };
            return Err("mmap /dev/fb0 failed".into());
        }

        let buf = unsafe {
            std::slice::from_raw_parts_mut(ptr as *mut u8, size)
        };

        Ok(Self { fd, buf, width, height, stride })
    }

    /// Write a single BGRA pixel. Silently clips out-of-bounds.
    #[inline(always)]
    pub fn set_pixel(&mut self, x: u32, y: u32, colour: Bgra) {
        if x >= self.width || y >= self.height { return; }
        let off = (y * self.stride + x * 4) as usize;
        self.buf[off]     = colour.b;
        self.buf[off + 1] = colour.g;
        self.buf[off + 2] = colour.r;
        self.buf[off + 3] = 0xFF;
    }

    /// Fill the entire screen with one colour.
    pub fn clear(&mut self, colour: Bgra) {
        for i in (0..self.buf.len()).step_by(4) {
            self.buf[i]     = colour.b;
            self.buf[i + 1] = colour.g;
            self.buf[i + 2] = colour.r;
            self.buf[i + 3] = 0xFF;
        }
    }

    /// Blit a 640-pixel-wide row slice from a world buffer row into the framebuffer.
    /// `src` must be BGRA bytes, length exactly `width * 4`.
    /// Used by WorldBuffer to copy the viewport each frame.
    pub fn blit_row(&mut self, screen_y: u32, src: &[u8]) {
        if screen_y >= self.height { return; }
        // Miyoo Mini Plus framebuffer is rotated 180 degrees
        let ry = self.height - 1 - screen_y;
        let dst_off = (ry * self.stride) as usize;
        let w = self.width as usize;
        let dst = &mut self.buf[dst_off..dst_off + w * 4];
        // Reverse pixels horizontally in one pass
        for x in 0..w {
            let rx = w - 1 - x;
            dst[rx * 4]     = src[x * 4];
            dst[rx * 4 + 1] = src[x * 4 + 1];
            dst[rx * 4 + 2] = src[x * 4 + 2];
            dst[rx * 4 + 3] = src[x * 4 + 3];
        }
    }

    /// Screen width in pixels (queried from hardware, not hardcoded).
    pub fn screen_w(&self) -> u32 { self.width }

    /// Screen height in pixels.
    pub fn screen_h(&self) -> u32 { self.height }
}

impl Drop for Framebuffer {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.buf.as_mut_ptr() as *mut _, self.buf.len());
            libc::close(self.fd);
        }
    }
}

// ── Bgra tests — these run on the dev machine ─────────────────────────────────

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

    // Framebuffer::open() is not tested here — it requires /dev/fb0
    // which only exists on the Miyoo. The blit and pixel logic is
    // tested via WorldBuffer in buffer.rs.
}

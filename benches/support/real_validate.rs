//! Untimed ABI and cross-library call checks for the real-library benchmarks.
use std::{
    ffi::{CStr, c_void},
    ptr,
};

#[derive(Clone, Copy)]
pub enum Workload {
    Zlib,
    Lz4,
    Zstd,
    Sqlite,
    Png,
    FreeType,
}

impl Workload {
    pub const ALL: &[Self] = &[
        Self::Zlib,
        Self::Lz4,
        Self::Zstd,
        Self::Sqlite,
        Self::Png,
        Self::FreeType,
    ];

    pub fn soname(self) -> &'static str {
        match self {
            Self::Zlib => "libz.so.1",
            Self::Lz4 => "liblz4.so.1",
            Self::Zstd => "libzstd.so.1",
            Self::Sqlite => "libsqlite3.so.0",
            Self::Png => "libpng16.so.16",
            Self::FreeType => "libfreetype.so.6",
        }
    }

    pub fn query(self) -> &'static [u8] {
        match self {
            Self::Zlib => b"zlibVersion",
            Self::Lz4 => b"LZ4_versionNumber",
            Self::Zstd => b"ZSTD_versionNumber",
            Self::Sqlite => b"sqlite3_libversion_number",
            Self::Png => b"png_access_version_number",
            Self::FreeType => b"FT_MulFix",
        }
    }

    // The caller owns the loaded graph for the entire validation call. Each
    // function is cast to its library's published C ABI, never a generic ABI.
    pub unsafe fn validate(self, get: impl Fn(&[u8]) -> *mut ()) -> String {
        match self {
            Self::Zlib => {
                let version: unsafe extern "C" fn() -> *const libc::c_char =
                    unsafe { std::mem::transmute(get(self.query())) };
                unsafe { CStr::from_ptr(version()) }
                    .to_str()
                    .unwrap()
                    .to_owned()
            }
            Self::FreeType => {
                // FT_Init_FreeType may dlopen optional HarfBuzz, which can retain
                // a native FreeType copy through a dependency cycle. This benchmark
                // measures DT_NEEDED loading, not application-created font engines.
                // FT_MulFix is the public FT_Long(FT_Long, FT_Long) arithmetic API.
                let mul: unsafe extern "C" fn(libc::c_long, libc::c_long) -> libc::c_long =
                    unsafe { std::mem::transmute(get(self.query())) };
                assert_eq!(unsafe { mul(10, 0x18000) }, 15);
                "FreeType arithmetic checked".to_owned()
            }
            Self::Png => {
                let version: unsafe extern "C" fn() -> u32 =
                    unsafe { std::mem::transmute(get(self.query())) };
                let version = unsafe { version() };
                unsafe { validate_png(&get) };
                format!("PNG {version}; decoded")
            }
            Self::Zstd => {
                let version: unsafe extern "C" fn() -> u32 =
                    unsafe { std::mem::transmute(get(self.query())) };
                unsafe { version() }.to_string()
            }
            Self::Lz4 | Self::Sqlite => {
                let version: unsafe extern "C" fn() -> i32 =
                    unsafe { std::mem::transmute(get(self.query())) };
                let version = unsafe { version() };
                assert!(version > 0);
                version.to_string()
            }
        }
    }
}

// The public libpng 1.6 simplified API layout:
// https://github.com/pnggroup/libpng/blob/libpng16/png.h
#[repr(C)]
struct PngImage {
    opaque: *mut c_void,
    version: u32,
    width: u32,
    height: u32,
    format: u32,
    flags: u32,
    colormap_entries: u32,
    warning_or_error: u32,
    message: [libc::c_char; 64],
}

unsafe fn validate_png(get: &impl Fn(&[u8]) -> *mut ()) {
    let begin: unsafe extern "C" fn(*mut PngImage, *const c_void, usize) -> i32 =
        unsafe { std::mem::transmute(get(b"png_image_begin_read_from_memory")) };
    let finish: unsafe extern "C" fn(
        *mut PngImage,
        *const c_void,
        *mut c_void,
        i32,
        *mut c_void,
    ) -> i32 = unsafe { std::mem::transmute(get(b"png_image_finish_read")) };
    let free: unsafe extern "C" fn(*mut PngImage) =
        unsafe { std::mem::transmute(get(b"png_image_free")) };
    let mut image: PngImage = unsafe { std::mem::zeroed() };
    image.version = 1;
    assert_eq!(
        unsafe { begin(&mut image, PNG.as_ptr().cast(), PNG.len()) },
        1
    );
    assert_eq!((image.width, image.height), (1, 1));
    image.format = 3; // PNG_FORMAT_RGBA = COLOR | ALPHA
    let mut rgba = [0u8; 4];
    let result = unsafe {
        finish(
            &mut image,
            ptr::null(),
            rgba.as_mut_ptr().cast(),
            0,
            ptr::null_mut(),
        )
    };
    unsafe { free(&mut image) };
    assert_eq!(result, 1);
    assert_eq!(rgba, [255, 0, 0, 255]);
}

// A generated 1x1 opaque red RGBA PNG, with zlib-compressed pixel data.
const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240, 31, 0,
    5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

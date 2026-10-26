use std::{ffi::c_void, io, ptr::NonNull};
use vecnook::{
    Result,
    generation::{SegmentFile, SegmentReader, Snapshot},
};

/// Replace a pinned generation's positional readers with read-only OS mappings.
///
/// The original SegmentFile and generation pin outlive each mapping. Core
/// publication/garbage collection never mutates or truncates a pinned segment.
/// No mapping is created for an empty generation.
///
/// # Safety
///
/// Every segment file must remain unchanged, including its length, until the
/// returned snapshot and its mapping sources are dropped. Use Publisher for all
/// storage operations; do not directly modify/truncate generation files or allow
/// non-cooperating processes to do so. Advisory locks cannot enforce this on Unix.
/// Corruption before mapping is checked; later external modification is excluded
/// by this contract. File mapping is opt-in rather than part of the safe core.
pub unsafe fn mapped_snapshot(snapshot: Snapshot) -> Result<Snapshot> {
    snapshot.with_segment_readers(|file| {
        // SAFETY: the caller guarantees file immutability for the returned view.
        unsafe { MappedSegment::new(file).map(|reader| Box::new(reader) as Box<dyn SegmentReader>) }
    })
}

struct MappedSegment {
    pointer: NonNull<u8>,
    length: usize,
    _source: SegmentFile,
    #[cfg(windows)]
    mapping: std::os::windows::io::RawHandle,
}
// SAFETY: no mutable pointers/slices are exposed. Mappings are read-only and
// remain alive (including their generation pin) for every bounded read.
unsafe impl Send for MappedSegment {}
// SAFETY: concurrent reads of the caller-guaranteed immutable mapping do not race.
unsafe impl Sync for MappedSegment {}

impl MappedSegment {
    unsafe fn new(source: SegmentFile) -> io::Result<Self> {
        let length = usize::try_from(source.len())
            .map_err(|_| io::Error::other("mapping length exceeds address space"))?;
        if length == 0 || length > isize::MAX as usize {
            return Err(io::Error::other("invalid mapping length"));
        }
        #[cfg(all(unix, target_pointer_width = "64"))]
        {
            use std::os::fd::AsRawFd;
            // SAFETY: source is an open read-only file of the validated size;
            // caller guarantees no external mutation. Offset zero is page aligned.
            let pointer = unsafe {
                unix::mmap(
                    std::ptr::null_mut(),
                    length,
                    1,
                    2,
                    source.file().as_raw_fd(),
                    0,
                )
            };
            if pointer as isize == -1 {
                return Err(io::Error::last_os_error());
            }
            let Some(pointer) = NonNull::new(pointer.cast::<u8>()) else {
                // SAFETY: mmap succeeded but returned address zero; release it
                // before rejecting the address, which NonNull cannot represent.
                unsafe { unix::munmap(pointer, length) };
                return Err(io::Error::other("mapping returned a null address"));
            };
            Ok(Self {
                pointer,
                length,
                _source: source,
            })
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            // SAFETY: file handle remains owned by source; PAGE_READONLY (2)
            // and zero size bind the mapping to the existing file length.
            let mapping = unsafe {
                windows::CreateFileMappingW(
                    source.file().as_raw_handle(),
                    std::ptr::null(),
                    2,
                    0,
                    0,
                    std::ptr::null(),
                )
            };
            if mapping.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: mapping is valid, FILE_MAP_READ (4), offset zero; length
            // is bounded by the validated immutable file.
            let pointer = unsafe { windows::MapViewOfFile(mapping, 4, 0, 0, length) };
            let Some(pointer) = NonNull::new(pointer.cast::<u8>()) else {
                let error = io::Error::last_os_error();
                // SAFETY: the successful mapping handle is not used after close.
                unsafe { windows::CloseHandle(mapping) };
                return Err(error);
            };
            Ok(Self {
                pointer,
                length,
                _source: source,
                mapping,
            })
        }
        #[cfg(not(any(all(unix, target_pointer_width = "64"), windows)))]
        {
            let _ = source;
            Err(io::ErrorKind::Unsupported.into())
        }
    }
}
impl SegmentReader for MappedSegment {
    fn len(&self) -> u64 {
        self.length as u64
    }
    fn read_exact_at(&self, offset: u64, destination: &mut [u8]) -> io::Result<()> {
        let offset = usize::try_from(offset).map_err(|_| io::ErrorKind::UnexpectedEof)?;
        if offset
            .checked_add(destination.len())
            .is_none_or(|end| end > self.length)
        {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        // SAFETY: bounds were checked; mapping remains immutable/alive. No mapped
        // memory is exposed to callers, so the mutable destination cannot overlap.
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.pointer.as_ptr().add(offset),
                destination.as_mut_ptr(),
                destination.len(),
            )
        };
        Ok(())
    }
}
impl Drop for MappedSegment {
    fn drop(&mut self) {
        #[cfg(all(unix, target_pointer_width = "64"))]
        // SAFETY: this object uniquely owns this complete, still-live mapping.
        unsafe {
            unix::munmap(self.pointer.as_ptr().cast(), self.length)
        };
        #[cfg(windows)]
        // SAFETY: view/handle were successfully created and are released once,
        // before _source drops its file and generation pin.
        unsafe {
            windows::UnmapViewOfFile(self.pointer.as_ptr().cast());
            windows::CloseHandle(self.mapping)
        };
    }
}
#[cfg(all(unix, target_pointer_width = "64"))]
mod unix {
    use super::c_void;
    unsafe extern "C" {
        pub fn mmap(
            address: *mut c_void,
            length: usize,
            protection: i32,
            flags: i32,
            fd: i32,
            offset: i64,
        ) -> *mut c_void;
        pub fn munmap(address: *mut c_void, length: usize) -> i32;
    }
}
#[cfg(windows)]
#[allow(non_snake_case)]
mod windows {
    use super::c_void;
    use std::os::windows::io::RawHandle;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn CreateFileMappingW(
            file: RawHandle,
            attributes: *const c_void,
            protection: u32,
            maximum_high: u32,
            maximum_low: u32,
            name: *const u16,
        ) -> RawHandle;
        pub fn MapViewOfFile(
            mapping: RawHandle,
            access: u32,
            offset_high: u32,
            offset_low: u32,
            length: usize,
        ) -> *mut c_void;
        pub fn UnmapViewOfFile(address: *const c_void) -> i32;
        pub fn CloseHandle(handle: RawHandle) -> i32;
    }
}

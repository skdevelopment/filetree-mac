//! macOS bulk directory enumeration via `getattrlistbulk` (APFS/HFS+ fast path).

use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct BulkEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub allocated: u64,
}

#[cfg(target_os = "macos")]
mod bulk {
    use super::*;
    use libc::{c_int, c_void, size_t};
    use std::mem::{align_of, size_of};

    const ATTR_BIT_MAP_COUNT: u16 = 5;
    const ATTR_CMN_NAME: u32 = 0x0000_0001;
    const ATTR_CMN_OBJTYPE: u32 = 0x0000_0002;
    const ATTR_CMN_PERM: u32 = 0x0000_0004;
    const ATTR_FILE_DATALENGTH: u32 = 0x0000_0001;
    const ATTR_FILE_ALLOCSIZE: u32 = 0x0000_0004;

    const VNODE_DIR: u32 = 1;
    const VNODE_REG: u32 = 2;
    const VNODE_LNK: u32 = 3;

    #[repr(C)]
    struct AttrList {
        bitmapcount: u16,
        reserved: u16,
        commonattr: u32,
        volattr: u32,
        dirattr: u32,
        fileattr: u32,
        forkattr: u32,
    }

    #[repr(C)]
    struct AttrReference {
        attr_data: [u8; 1],
    }

    extern "C" {
        fn getattrlistbulk(
            dirfd: c_int,
            attr_list: *const AttrList,
            attr_buf: *mut c_void,
            attr_buf_size: size_t,
            options: u64,
        ) -> c_int;
    }

    pub fn read_dir_bulk(parent: &Path) -> io::Result<Vec<BulkEntry>> {
        let file = fs::File::open(parent)?;
        let fd = file.as_raw_fd();
        let attr_list = AttrList {
            bitmapcount: ATTR_BIT_MAP_COUNT,
            reserved: 0,
            commonattr: ATTR_CMN_NAME | ATTR_CMN_OBJTYPE | ATTR_CMN_PERM,
            volattr: 0,
            dirattr: 0,
            fileattr: ATTR_FILE_DATALENGTH | ATTR_FILE_ALLOCSIZE,
            forkattr: 0,
        };

        const BUF_SIZE: usize = 64 * 1024;
        let mut buf = vec![0u8; BUF_SIZE];
        let mut entries = Vec::new();

        loop {
            let count = unsafe {
                getattrlistbulk(
                    fd,
                    &attr_list as *const AttrList,
                    buf.as_mut_ptr() as *mut c_void,
                    BUF_SIZE,
                    0,
                )
            };
            if count <= 0 {
                if count == 0 {
                    break;
                }
                return Err(io::Error::last_os_error());
            }

            let mut offset = 0usize;
            for _ in 0..count as usize {
                if offset + size_of::<u32>() > buf.len() {
                    break;
                }
                let rec_len =
                    u32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap()) as usize;
                if rec_len < 4 || offset + rec_len > buf.len() {
                    break;
                }
                let rec = &buf[offset..offset + rec_len];
                offset += rec_len;

                if let Some(entry) = parse_record(parent, rec) {
                    if entry.name != "." && entry.name != ".." {
                        entries.push(entry);
                    }
                }
            }
        }

        Ok(entries)
    }

    fn parse_record(parent: &Path, rec: &[u8]) -> Option<BulkEntry> {
        let mut pos = 4usize;
        let mut name: Option<String> = None;
        let mut objtype: u32 = 0;
        let mut size: u64 = 0;
        let mut allocated: u64 = 0;

        let attr_list = AttrList {
            bitmapcount: ATTR_BIT_MAP_COUNT,
            reserved: 0,
            commonattr: ATTR_CMN_NAME | ATTR_CMN_OBJTYPE | ATTR_CMN_PERM,
            volattr: 0,
            dirattr: 0,
            fileattr: ATTR_FILE_DATALENGTH | ATTR_FILE_ALLOCSIZE,
            forkattr: 0,
        };

        if attr_list.commonattr & ATTR_CMN_NAME != 0 {
            if pos + size_of::<u32>() > rec.len() {
                return None;
            }
            let name_len = u32::from_le_bytes(rec[pos..pos + 4].try_into().ok()?) as usize;
            pos += size_of::<u32>();
            let aligned = align_up(pos, align_of::<AttrReference>());
            pos = aligned;
            if pos + name_len > rec.len() {
                return None;
            }
            let name_bytes = &rec[pos..pos + name_len];
            pos += name_len;
            name = std::str::from_utf8(name_bytes).ok().map(|s| s.to_string());
        }

        if attr_list.commonattr & ATTR_CMN_OBJTYPE != 0 {
            pos = align_up(pos, align_of::<u32>());
            if pos + 4 > rec.len() {
                return None;
            }
            objtype = u32::from_le_bytes(rec[pos..pos + 4].try_into().ok()?);
            pos += 4;
        }

        if attr_list.commonattr & ATTR_CMN_PERM != 0 {
            pos = align_up(pos, align_of::<u32>());
            pos += 4;
        }

        if attr_list.fileattr & ATTR_FILE_DATALENGTH != 0 {
            pos = align_up(pos, align_of::<u64>());
            if pos + 8 <= rec.len() {
                size = u64::from_le_bytes(rec[pos..pos + 8].try_into().ok()?);
                pos += 8;
            }
        }

        if attr_list.fileattr & ATTR_FILE_ALLOCSIZE != 0 {
            pos = align_up(pos, align_of::<u64>());
            if pos + 8 <= rec.len() {
                allocated = u64::from_le_bytes(rec[pos..pos + 8].try_into().ok()?);
            }
        }

        let name = name?;
        let is_symlink = objtype == VNODE_LNK;
        let is_dir = objtype == VNODE_DIR;
        let path = parent.join(&name);
        Some(BulkEntry {
            name,
            path,
            is_dir,
            is_symlink,
            size: if is_dir { 0 } else { size },
            allocated: if is_dir { 0 } else { allocated },
        })
    }

    fn align_up(pos: usize, align: usize) -> usize {
        (pos + align - 1) & !(align - 1)
    }
}

#[cfg(not(target_os = "macos"))]
mod bulk {
    use super::*;

    pub fn read_dir_bulk(_parent: &Path) -> io::Result<Vec<BulkEntry>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "getattrlistbulk is macOS-only",
        ))
    }
}

/// Read directory entries with a single stat per file when possible.
pub fn read_dir_fast(parent: &Path, show_hidden: bool) -> io::Result<Vec<BulkEntry>> {
    #[cfg(target_os = "macos")]
    {
        if let Ok(entries) = bulk::read_dir_bulk(parent) {
            if show_hidden {
                return Ok(entries);
            }
            return Ok(entries
                .into_iter()
                .filter(|e| !e.name.starts_with('.'))
                .collect());
        }
    }

    read_dir_fallback(parent, show_hidden)
}

fn read_dir_fallback(parent: &Path, show_hidden: bool) -> io::Result<Vec<BulkEntry>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let file_type = entry.file_type()?;
        let is_symlink = file_type.is_symlink();
        let is_dir = if is_symlink {
            false
        } else {
            file_type.is_dir()
        };

        let (size, allocated) = if is_dir {
            (0, 0)
        } else {
            match fs::symlink_metadata(&path) {
                Ok(meta) => (meta.size(), meta.blocks() * 512),
                Err(_) => (0, 0),
            }
        };

        entries.push(BulkEntry {
            name,
            path,
            is_dir,
            is_symlink,
            size,
            allocated,
        });
    }
    Ok(entries)
}

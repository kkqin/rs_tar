use std::mem::size_of;
use std::ptr::read_unaligned;
use std::io;

const T_BLOCKSIZE : usize = 512;

#[repr(u32)] // 确保底层表示是 u32 类型
pub enum TarFileType {
    Undefined = 0x00,
    Regular = 0x01,
    Directory = 0x02,
    Fifo = 0x03,
    CharacterDevice = 0x04,
    BlockDevice = 0x05,
    SymbolicLink = 0x06,
    Shadow = 0x07, // SOLARIS ONLY
    UnixDomainSocket = 0x08,
    Whiteout = 0x09,
    VirtualFile = 0x0a, // Virtual File created by TSK for file system areas
    VirtualDirectory = 0x0b, // Virtual Directory created by TSK to hold data like orphan files
}


#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TarHeader {
    pub name: [u8; 100],
    pub mode: [u8; 8],
    pub uid: [u8; 8],
    pub gid: [u8; 8],
    pub size: [u8; 12],
    pub mtime: [u8; 12],
    pub chksum: [u8; 8],
    pub typeflag: u8,
    pub linkname: [u8; 100],
    pub magic: [u8; 6],
    pub version: [u8; 2],
    pub uname: [u8; 32],
    pub gname: [u8; 32],
    pub devmajor: [u8; 8],
    pub devminor: [u8; 8],
    pub prefix: [u8; 155],
    pub padding: [u8; 12],
}

pub unsafe fn read_tar_header(buf: &[u8]) -> io::Result<TarHeader> {
    assert!(buf.len() >= size_of::<TarHeader>());
    let ptr = buf.as_ptr() as *const TarHeader;
    let hdr = read_unaligned(ptr);
    Ok(hdr)
}

impl TarHeader {
    pub fn get_uname(&self) -> String {
        match std::str::from_utf8(&self.uname) {
            Ok(s) => s.trim_end_matches('\0').to_string(),
            Err(_) => String::new(),
        }
    }

    pub fn get_gname(&self) -> String {
        match std::str::from_utf8(&self.gname) {
            Ok(s) => s.trim_end_matches('\0').to_string(),
            Err(_) => String::new(),
        }
    }

    /// 从 tar header 中读取 size 字段
    pub fn get_size(&self) -> u64 {
        Self::parse_octal(&self.size)
    }

    /// 从 tar header 中读取 uid 字段
    pub fn get_uid(&self) -> u64 {
        Self::parse_octal(&self.uid)
    }

    /// 从 tar header 中读取 gid 字段
    pub fn get_gid(&self) -> u64 {
        Self::parse_octal(&self.gid)
    }

    /// 从 tar header 中读取修改时间（mtime）字段
    pub fn get_mtime(&self) -> u64 {
        Self::parse_octal(&self.mtime)
    }

    /// 公共方法：从一个 `[u8]` 八进制字段解析成 u64
    fn parse_octal(field: &[u8]) -> u64 {
        match std::str::from_utf8(field) {
            Ok(s) => {
                let s = s.trim_end_matches('\0').trim();
                u64::from_str_radix(s, 8).unwrap_or(0)
            }
            Err(_) => 0,
        }
    }

    pub fn get_name(&self) -> String {
        match std::str::from_utf8(&self.name) {
            Ok(s) => s.trim_end_matches('\0').to_string(),
            Err(_) => String::new(),
        }
    }

    pub fn get_prefix(&self) -> String {
        match std::str::from_utf8(&self.prefix) {
            Ok(s) => s.trim_end_matches('\0').to_string(),
            Err(_) => String::new(),
        }
    }

    /// 获取完整路径（prefix + name），如果 prefix 存在
    pub fn get_full_path(&self) -> String {
        let prefix = self.get_prefix();
        let name = self.get_name();
        if !prefix.is_empty() {
            format!("{}/{}", prefix, name)
        } else {
            name
        }
    }

    pub fn get_type_flag(&self) -> char {
        self.typeflag as char
    }

    pub fn get_link_name(&self) -> String {
        match std::str::from_utf8(&self.linkname) {
            Ok(s) => s.trim_end_matches('\0').to_string(),
            Err(_) => String::new(),
        }
    }

        /// 检查 checksum 是否正确
    pub fn crc_ok(&self) -> bool {
        let real_crc = self.get_crc();
        real_crc == self.crc_calc() || real_crc == self.signed_crc_calc()
    }

    pub fn crc_calc(&self) -> i32 {
        let ptr = self as *const _ as *const u8;
        let buf = unsafe { std::slice::from_raw_parts(ptr, T_BLOCKSIZE) };

        let mut sum = 0i32;
        for &b in buf {
            sum += b as i32;
        }
        for &b in &self.chksum {
            sum -= b as i32;
        }
        sum + (' ' as i32) * (self.chksum.len() as i32)
    }

    /// 计算 signed 校验和
    pub fn signed_crc_calc(&self) -> i32 {
        let ptr = self as *const _ as *const i8;
        let buf = unsafe { std::slice::from_raw_parts(ptr, T_BLOCKSIZE) };

        let mut sum = 0i32;
        for &b in buf {
            sum += b as i32;
        }
        for &b in &self.chksum {
            sum += (' ' as i8 - b as i8) as i32;
        }
        sum
    }

    /// 获取 header 中保存的 checksum
    pub fn get_crc(&self) -> i32 {
        Self::parse_octal(&self.chksum) as i32
    }

}


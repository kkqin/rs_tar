use std::{fs::File, io::{self, Read, Seek, SeekFrom}, sync::Arc};
use crate::tar::{TarHeader, read_tar_header};

/// 文件信息行为抽象，继承 Read + Seek
pub trait FileInfo: Read + Seek {}

/// 镜像信息抽象接口
pub trait ImageInfo: Sized + Read + Seek {
    /// 打开一个镜像并返回智能指针
    fn open(path: &str) -> io::Result<Arc<Self>>;
    /// 获取镜像文件总大小
    fn get_size(&self) -> io::Result<u64>;
    /// 根据偏移和长度打开文件片段
    fn open_file_at(&mut self, offset: u64, size: u64) -> io::Result<(Box<dyn FileInfo>, u64)>;
    /// 遍历所有条目，并在每个条目上调用回调
    fn for_each_entry<F>(&mut self, callback: F) -> io::Result<()>
    where
        F: FnMut(Box<dyn FileInfo>) -> io::Result<()>;
}

/// Tar 镜像实现，只保存路径
#[derive(Clone)]
pub struct TarImage {
    file: Arc<File>,
    path: String,
    size: u64,
    last_link_name : String,
}

impl Read for TarImage {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut file = self.file.as_ref().try_clone()?;
        file.read(buf)
    }
}

impl Seek for TarImage {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let mut file = self.file.as_ref().try_clone()?;
        file.seek(pos)
    }
}

impl ImageInfo for TarImage {
    fn open(path: &str) -> io::Result<Arc<Self>> {
        let file = Arc::new(File::open(path)?);
        let size = file.metadata()?.len();
        Ok(Arc::new(TarImage {
            file,
            path: path.to_string(),
            size,
            last_link_name: String::new(),
        }))
    }

    fn get_size(&self) -> io::Result<u64> {
        Ok(self.size)
    }

    fn open_file_at(&mut self, offset: u64, size: u64) -> io::Result<(Box<dyn FileInfo>,u64)> {
        self.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; size as usize];
        self.read(&mut buf)?;
        // 在 unsafe 外部声明并使用 hdr
        let hdr_res: (TarHeader, u64);
        unsafe {
            hdr_res = match read_tar_header(&buf) {
                Ok(res) => res,
                Err(e) => return Err(e),
            };
        }
        let (hdr, n) = hdr_res;
        let mut tar_file = TarFile::new(Arc::new(self.clone()), hdr);
        tar_file.base_offset = offset;
        if hdr.get_type_flag() == '5' {
            tar_file.file_type = TarFileType::Directory as i32;
        } else if hdr.get_type_flag() == '1' {
            tar_file.file_type = TarFileType::SymbolicLink as i32;
            if self.last_link_name != "" {
                tar_file.link = self.last_link_name.clone();
            }
        } else if hdr.get_type_flag() == 'K' {
            self.last_link_name = hdr.get_link_name();
        }

        Ok((Box::new(tar_file),n))
    }

    fn for_each_entry<F>(&mut self, mut callback: F) -> io::Result<()>
    where
        F: FnMut(Box<dyn FileInfo>) -> io::Result<()>,
    {
        let mut off: u64 = 0;
        while off < self.size {
            match read_file_header(self, off) {
                Ok(file_res) => {
                    let (file, n) = file_res;
                    callback(file)?;
                    off += n;
                },
                Err(e) => {
                    eprintln!("Error reading file header: {}", e);
                    return Err(e);
                }
            };
        }
        Ok(())
    }
}

fn read_file_header(img_info :&mut TarImage, offset:u64) -> io::Result<(Box<dyn FileInfo>, u64)> {
    let file_res = img_info.open_file_at(offset, 512)?;
    Ok(file_res)
}

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


/// Tar 文件片段结构，包含镜像引用、起始偏移和结束偏移
pub struct TarFile {
    image: Arc<TarImage>,
    header : TarHeader,
    base_offset: u64,
    pos: u64,
    file_type: i32,
    link : String,
}

impl TarFile {
    pub fn new(image: Arc<TarImage>, hdr: TarHeader) -> Self {
        TarFile {
            image,
            header: hdr,
            base_offset: 0,
            pos: 0,
            file_type: -1,
            link: String::new(),
        }
    }
}

impl Read for TarFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        /*if self.pos >= self.end_offset {
            return Ok(0);
        }
        let remaining = (self.end_offset - self.pos) as usize;
        let to_read = buf.len().min(remaining);
        let mut file = self.image.file.as_ref().try_clone()?;
        file.seek(SeekFrom::Start(self.pos))?;
        let n = file.read(&mut buf[..to_read])?;
        self.pos += n as u64;*/
        Ok(0)
    }
}

impl Seek for TarFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let new_pos = match pos {
            SeekFrom::Start(n) => self.base_offset.saturating_add(n),
            SeekFrom::End(n) => (self.pos + self.header.get_size()).saturating_add(n.try_into().unwrap()) as u64,
            SeekFrom::Current(n) => (self.pos as i64).saturating_add(n) as u64,
        };
        /*if new_pos < self.base_offset || new_pos > self.end_offset {
            Err(io::Error::new(io::ErrorKind::InvalidInput, "seek out of range"))
        } else {
            self.pos = new_pos;
            Ok(self.pos - self.base_offset)
        }*/
        Err(io::Error::new(io::ErrorKind::InvalidInput, "not implemented"))
    }
}

/// 将 TarFile 标记为 FileInfo
impl FileInfo for TarFile {}

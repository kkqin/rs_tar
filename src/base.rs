use std::{fs::File, io::{self, Read, Seek, SeekFrom}, sync::{Arc, Mutex}};
use crate::tar::{TarHeader, read_tar_header, TarFileType};
use std::any::Any;

/// 文件信息行为抽象，继承 Read + Seek
pub trait FileInfo: Read + Seek + Any {
    fn as_any(&self) -> &dyn Any;
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}

/// 镜像信息抽象接口
pub trait ImageInfo: Sized + Read + Seek {
    /// 打开一个镜像并返回智能指针
    fn open(path: &str) -> io::Result<Arc<Mutex<Self>>>;
    /// 获取镜像文件总大小
    fn get_size(&self) -> io::Result<u64>;
    fn read_img_at(&mut self, offset: u64, size: u64) -> io::Result<(Vec<u8>, u64)>;
    fn get_file_at(&mut self, offset: u64) -> io::Result<(Box<dyn FileInfo>,u64)>;
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

impl TarImage {
    pub fn get_path(&self) -> String {
        self.path.clone()
    }
}

impl ImageInfo for TarImage {
    fn open(path: &str) -> io::Result<Arc<Mutex<Self>>> {
        let file = Arc::new(File::open(path)?);
        let size = file.metadata()?.len();
        Ok(Arc::new(Mutex::new(TarImage {
            file,
            path: path.to_string(),
            size,
            last_link_name: String::new(),
        })))
    }

    fn get_size(&self) -> io::Result<u64> {
        Ok(self.size)
    }

    fn read_img_at(&mut self, offset: u64, size: u64) -> io::Result<(Vec<u8>, u64)> {
        let mut file = self.file.as_ref().try_clone()?;
        file.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; size as usize];
        let n = file.read(&mut buf)?;
        if n != size as usize {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "not enough data"));
        }
        Ok((buf, n as u64))
    }

    fn get_file_at(&mut self, offset: u64) -> io::Result<(Box<dyn FileInfo>,u64)> {
        match read_file_header(self, offset) {
            Ok(file_res) => {
                return Ok(file_res);
            },
            Err(e) => {
                return Err(e);
            }
        };
    }

    fn for_each_entry<F>(&mut self, mut callback: F) -> io::Result<()>
    where
        F: FnMut(Box<dyn FileInfo>) -> io::Result<()>,
    {
        let mut off: u64 = 0;
        while off < self.size {
            match read_file_header(self, off) {
                Ok((file,n)) => {
                    let tar_file = try_into_tarfile(file)?;
                    let mut body_size = tar_file.header.get_size();
                    body_size = if (body_size % 512) == 0 {
                        body_size
                    } else {
                        ((body_size / 512) + 1) *512
                    };
                    if tar_file.header.get_type_flag() == 'K' {
                        off += tar_file.header_size;
                    } else {
                        off += n + body_size;
                    }
                    if tar_file.header.get_type_flag() != 'K' {
                        callback(tar_file)?;
                    }
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

/// 从 TarImage 读取 header 并返回 (header, total_header_size)
pub fn tar_hdr_read_internal(img_info: &mut TarImage, offset: u64) -> io::Result<(TarHeader, u64)> {
    const BLOCK_SIZE: u64 = 512;
    let mut header_size: u64 = 0;
    let mut num_zero_blocks: u32 = 0;

    loop {
        // 读取一个 512 字节块
        let (buf, n) = img_info.read_img_at(offset + header_size, BLOCK_SIZE)
            .map_err(|e| io::Error::new(e.kind(), format!("Error reading image at offset {}: {}", offset + header_size, e)))?;
        if n < BLOCK_SIZE {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "not enough data"));
        }

        // 解析 tar header
        let hdr   = unsafe { read_tar_header(&buf)? };
        header_size += BLOCK_SIZE;

        // 检测全零块 (EOF)
        if hdr.get_name().is_empty() {
            num_zero_blocks += 1;
            if num_zero_blocks >= 2 {
                // 两个全零块表示真正的 EOF，返回 size = 0
                return Ok((hdr, 0));
            } else {
                // 第一个全零块，继续循环
                continue;
            }
        }

        // 验证 checksum
        if !hdr.crc_ok() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "tar header checksum error"));
        }

        // 成功解析到有效 header，返回 header 和已读取的大小
        return Ok((hdr, header_size));
    }
}

fn read_file_header(img_info :&mut TarImage, offset:u64) -> io::Result<(Box<dyn FileInfo>, u64)> {
    let (hdr, n) = tar_hdr_read_internal(img_info, offset)?;
    let mut tar_file = TarFile::new(Arc::new(img_info.clone().into()), hdr);
    tar_file.base_offset = offset;
    if hdr.get_type_flag() == '5' {
        tar_file.file_type = TarFileType::Directory as i32;
    } else if hdr.get_type_flag() == '1' {
        tar_file.file_type = TarFileType::SymbolicLink as i32;
        if img_info.last_link_name != "" {
            tar_file.link = img_info.last_link_name.clone();
        }
    } else if hdr.get_type_flag() == 'K' {
        img_info.last_link_name = hdr.get_link_name();
    }
    if n == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "tar header size is zero"));
    }
    tar_file.header_size = n;
    Ok((Box::new(tar_file),n))
}


/// Tar 文件片段结构，包含镜像引用、起始偏移和结束偏移
#[derive(Clone)]
pub struct TarFile {
    image: Arc<Mutex<TarImage>>,
    header : TarHeader,
    base_offset: u64,
    pos: u64,
    file_type: i32,
    link : String,
    header_size: u64,
}

impl TarFile {
    pub fn new(image: Arc<Mutex<TarImage>>, hdr: TarHeader) -> Self {
        TarFile {
            image,
            header: hdr,
            base_offset: 0,
            pos: 0,
            file_type: -1,
            link: String::new(),
            header_size: 0,
        }
    }
}

impl Read for TarFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut img = self.image.try_lock().map_err(|_| {
            io::Error::new(io::ErrorKind::Other, "Failed to lock TarImage")
        })?;
        if self.pos >= self.header.get_size() {
            return Ok(0);
        }
        img.seek(SeekFrom::Start(self.pos))?;
        Ok(img.read(buf).map(|n| {
            self.pos += n as u64;
            n
        })?)
    }
}

impl Seek for TarFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let mut img = self.image.try_lock().map_err(|_| {
            io::Error::new(io::ErrorKind::Other, "Failed to lock TarImage")
        })?;
        let new_pos = match pos {
            SeekFrom::Start(n) => SeekFrom::Start(self.base_offset + n),
            SeekFrom::End(n) => SeekFrom::End(n),
            SeekFrom::Current(n) => SeekFrom::Current(n),
        };
        Ok(img.seek(new_pos)?)
    }
}

/// 将 TarFile 标记为 FileInfo
impl FileInfo for TarFile {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

impl TarFile {
    pub fn get_name(&self) -> String {
        self.header.get_name()
    }
    pub fn get_size(&self) -> u64 {
        self.header.get_size()
    }
    pub fn get_type_flag(&self) -> char {
        self.header.get_type_flag()
    }
    pub fn get_offset(&self) -> u64 {
        self.base_offset
    }
}

pub fn try_into_tarfile(b: Box<dyn FileInfo>) -> io::Result<Box<TarFile>> {
    b.into_any().downcast::<TarFile>().map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "Type is not TarFile")
    })
}

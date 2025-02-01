use super::ElfObject;
use std::{ffi::CString, io::BufReader, net::TcpStream};

pub struct ElfStream {
    name: CString,
    stream: BufReader<TcpStream>,
}

impl ElfStream {
    pub fn new(stream: TcpStream, name: &str) -> Self {
        Self {
            name: CString::new(name).unwrap(),
            stream: BufReader::new(stream),
        }
    }
}

impl ElfObject for ElfStream {
    fn file_name(&self) -> &core::ffi::CStr {
        &self.name
    }

    fn read(&mut self, buf: &mut [u8], offset: usize) -> crate::Result<()> {
		// TODO: 需要为tcpstream实现类似seek的效果，尽量避免无效的拷贝。
		// BufReader好像无法实现类似的效果，可能需要寻找一些实现类似功能的库，或者自己写一个Buffer?
		todo!();
    }

    fn as_fd(&self) -> Option<i32> {
        None
    }
}

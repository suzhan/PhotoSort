//! 扫描阶段收集的文件信息（轻量，不读取文件内容）。

use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct PhotoFile {
    pub path: PathBuf,
    pub size: u64,
    /// 小写、不含点，如 "jpg"。
    pub extension: String,
    pub modified_time: SystemTime,
}

impl PhotoFile {
    /// 展示用文件名（容忍非 UTF-8）。
    pub fn file_name_lossy(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_name_lossy_works() {
        let f = PhotoFile {
            path: PathBuf::from("/photos/DSC_1231.JPG"),
            size: 1024,
            extension: "jpg".to_string(),
            modified_time: SystemTime::UNIX_EPOCH,
        };
        assert_eq!(f.file_name_lossy(), "DSC_1231.JPG");
        assert_eq!(f.size, 1024);
    }
}

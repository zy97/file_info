use clap::Parser;
use md5::{Digest, Md5};
use rayon::prelude::*;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;

/// 文件信息工具 - 分析指定路径的文件信息
#[derive(Parser, Debug)]
#[command(name = "file_info")]
#[command(author, version, about, long_about = None)]
struct Args {
    /// 要分析的路径
    #[arg(value_name = "PATH")]
    path: PathBuf,

    /// 要忽略的路径列表(可多次指定)
    #[arg(short, long, value_name = "IGNORE_PATH")]
    ignore: Vec<PathBuf>,
}

fn should_ignore(path: &Path, ignore_paths: &[PathBuf]) -> bool {
    for ignore_path in ignore_paths {
        if path.starts_with(ignore_path) {
            return true;
        }
    }
    false
}

fn get_timestamp(system_time: SystemTime) -> u64 {
    system_time
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn calculate_md5(file_path: &Path) -> io::Result<String> {
    let mut file = File::open(file_path)?;
    let mut hasher = Md5::new();
    let mut buffer = [0; 8192];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn main() {
    let args = Args::parse();

    println!("分析路径: {}", args.path.display());

    if !args.ignore.is_empty() {
        println!("忽略路径:");
        for ignore_path in &args.ignore {
            println!("  - {}", ignore_path.display());
        }
    }

    println!("\n文件列表及更新时间:");
    println!("{:-<80}", "");

    // 收集所有文件路径
    let files: Vec<_> = WalkDir::new(&args.path)
        .into_iter()
        .filter_entry(|e| !should_ignore(e.path(), &args.ignore))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .collect();

    // 并行处理文件
    files.par_iter().for_each(|entry| match entry.metadata() {
        Ok(metadata) => match metadata.modified() {
            Ok(modified) => {
                let timestamp = get_timestamp(modified);
                match calculate_md5(entry.path()) {
                    Ok(md5_hash) => {
                        println!("{}\t{}\t{}", timestamp, md5_hash, entry.path().display());
                    }
                    Err(e) => {
                        eprintln!("无法计算MD5 {}: {}", entry.path().display(), e);
                    }
                }
            }
            Err(e) => {
                eprintln!("无法获取修改时间 {}: {}", entry.path().display(), e);
            }
        },
        Err(e) => {
            eprintln!("无法读取元数据 {}: {}", entry.path().display(), e);
        }
    });
}
